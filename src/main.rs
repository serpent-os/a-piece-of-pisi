// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::{fs::File, io::Cursor, time::Duration};

use a_piece_of_pisi::eopkg::{self, index::Package};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lzma::LzmaReader;
use reqwest::Url;
use serde_xml_rs::from_reader;

use futures::future::try_join_all;
use thiserror::Error;
use url::ParseError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("uri parse: {0}")]
    URI(#[from] ParseError),

    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("io: {0}")]
    IO(#[from] std::io::Error),

    #[error("invalid uri")]
    InvalidURI,
}

/// Unfriendly fetch routine with no proper error handling.
async fn fetch(multi: &MultiProgress, p: &Package) -> Result<(), Error> {
    let full_url = format!("https://packages.getsol.us/unstable/{}", &p.package_uri);
    let uri = Url::parse(&full_url)?;
    let path = uri
        .path_segments()
        .ok_or(Error::InvalidURI)?
        .last()
        .ok_or(Error::InvalidURI)?
        .to_string();
    let mut r = reqwest::get(uri).await?;
    let pbar = multi.add(ProgressBar::new(p.package_size));
    pbar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}]  {bar:20.cyan/blue}  {bytes:>7}/{total_bytes:7} {wide_msg:>.dim}",
        )
        .unwrap()
        .progress_chars("##-"),
    );
    pbar.set_message(path.clone());
    pbar.enable_steady_tick(Duration::from_millis(150));

    let mut output = File::create(path).unwrap();

    while let Some(chunk) = &r.chunk().await? {
        let mut cursor = Cursor::new(chunk);
        let len = chunk.len();
        std::io::copy(&mut cursor, &mut output)?;
        pbar.inc(len as u64);
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let multi = MultiProgress::new();

    let bytes = include_bytes!("../test/eopkg-index.xml.xz");
    let cursor = Cursor::new(bytes);
    let reader = LzmaReader::new_decompressor(cursor).unwrap();
    let xml_bar = ProgressBar::new_spinner();
    xml_bar.enable_steady_tick(Duration::from_millis(150));
    xml_bar.set_message("Loading eopkg-index.xml.xz");
    let doc: eopkg::index::Index = from_reader(xml_bar.wrap_read(reader)).unwrap();
    xml_bar.finish_and_clear();

    let pkgs = doc
        .packages
        .iter()
        .filter(|p| p.source.name == "glibc")
        .collect::<Vec<_>>();
    try_join_all(pkgs.iter().map(|m| async { fetch(&multi, m).await }))
        .await
        .unwrap();
}
