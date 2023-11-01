// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::{collections::BTreeSet, fs::File, io::Cursor, time::Duration};

use a_piece_of_pisi::{
    converter::{convert, HashedPackage},
    eopkg::{
        self,
        index::{Index, Package},
    },
};
use crossterm::style::Stylize;
use indicatif::{style::TemplateError, MultiProgress, ProgressBar, ProgressStyle};
use lzma::LzmaReader;
use reqwest::Url;
use serde_xml_rs::from_reader;

use futures::{stream, StreamExt, TryStreamExt};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::ParseError;

use color_eyre::Result;

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

    #[error("invalid template: {0}")]
    Template(#[from] TemplateError),
}

/// Asynchronously fetch a package (TODO: Stop hardcoding the origin URI base!)
async fn fetch(multi: &MultiProgress, p: &Package) -> Result<HashedPackage, Error> {
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
        )?
        .progress_chars("##-"),
    );
    pbar.set_message(path.clone());
    pbar.enable_steady_tick(Duration::from_millis(150));

    let mut hasher = Sha256::new();
    let mut output = File::create(&path).unwrap();

    while let Some(chunk) = &r.chunk().await? {
        let mut cursor = Cursor::new(chunk);
        let len = chunk.len();
        std::io::copy(&mut cursor, &mut output)?;
        pbar.inc(len as u64);
        hasher.update(chunk);
    }
    let hash = hasher.finalize();

    pbar.println(format!("{} {}", "Fetched".green(), path.clone().bold()));
    Ok(HashedPackage {
        package: p.clone(),
        hash: hash.into(),
    })
}

async fn parse_index() -> Result<Index, Error> {
    let bytes = include_bytes!("../test/eopkg-index.xml.xz");
    let cursor = Cursor::new(bytes);
    let xml_bar = ProgressBar::new(bytes.len() as u64);
    xml_bar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}]  {bar:20.red/white}  {bytes:>7}/{total_bytes:7} {wide_msg:>.dim}",
        )?
        .progress_chars("##-"),
    );
    xml_bar.enable_steady_tick(Duration::from_millis(150));
    xml_bar.set_message("Loading eopkg-index.xml.xz");

    let reader = LzmaReader::new_decompressor(xml_bar.wrap_read(cursor)).unwrap();
    let doc: eopkg::index::Index = from_reader(reader).unwrap();
    xml_bar.println(format!(
        "{} {}",
        "Loaded".blue(),
        "eopkg-index.xml.xz".bold()
    ));
    xml_bar.finish_and_clear();

    Ok(doc)
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let multi = MultiProgress::new();
    let index = parse_index().await?;

    // TODO: create a proper table.
    let names = index
        .packages
        .iter()
        .map(|p| p.source.name.clone())
        .collect::<BTreeSet<String>>();
    eprintln!("Unique source IDs: {}", names.len());

    let results: Vec<HashedPackage> = stream::iter(
        index
            .packages
            .iter()
            .filter(|p| p.source.name == "nano")
            .map(|f| async { fetch(&multi, f).await }),
    )
    .buffer_unordered(16)
    .try_collect()
    .await?;

    println!("YML:\n\n");
    println!(
        "{}",
        convert(results, Url::parse("https://packages.getsol.us/unstable")?)?
    );
    Ok(())
}
