// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::{collections::BTreeSet, ffi::OsStr, fs::File, io::Cursor, path::PathBuf, time::Duration};

use a_piece_of_pisi::eopkg::{
    self,
    index::{Index, Package},
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

#[derive(Default)]
struct FetchedPackage {
    package: Package,
    hash: String,
}

/// Asynchronously fetch a package (TODO: Stop hardcoding the origin URI base!)
async fn fetch(multi: &MultiProgress, p: &Package) -> Result<FetchedPackage, Error> {
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
    Ok(FetchedPackage {
        package: p.clone(),
        hash: const_hex::encode(hash),
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

fn generate_install_script<'a, T: IntoIterator<Item = &'a FetchedPackage>>(pkgs: T) -> String {
    let pkgs = pkgs.into_iter();
    let script = "    %install_dir %(installroot)";
    let zips = pkgs
        .map(|p| {
            let path = PathBuf::from(&p.package.package_uri);
            format!(
                "    unzip -o %(sourcedir)/{}\n    tar xf install.tar.xz -C %(installroot)",
                path.file_name()
                    .unwrap_or(OsStr::new("no-exist.eopkg"))
                    .to_string_lossy()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}\n{}", script, zips)
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

    let results: Vec<FetchedPackage> = stream::iter(
        index
            .packages
            .iter()
            .take(300)
            .map(|f| async { fetch(&multi, f).await }),
    )
    .buffer_unordered(16)
    .try_collect()
    .await?;
    let sample = results.first().unwrap();
    let upstreams = results.iter().map(|p| {
        format!(
            " - https://packages.getsol.us/unstable/{}:\n    unpack: false\n    hash: {}",
            p.package.package_uri, p.hash
        )
    });
    let homepage = sample
        .package
        .source
        .homepage
        .clone()
        .unwrap_or("no-homepage-set".to_string());
    let mut yml = vec![
        format!("name: {}", sample.package.source.name),
        format!("version: {}", sample.package.history.updates[0].version),
        format!("release: {}", sample.package.history.updates[0].release),
        format!("summary: {}", sample.package.summary),
        format!("description: {}", sample.package.description),
        format!("homepage: {}", homepage),
        "strip: false".to_string(),
        "license: ".to_string(),
    ];
    yml.extend(
        sample
            .package
            .licenses
            .iter()
            .map(|l| format!("    - {}", l)),
    );
    yml.push("upstreams:".to_string());
    yml.extend(upstreams);
    let steps = vec!["install: |".to_string(), generate_install_script(&results)];
    yml.extend(steps);
    for i in yml {
        println!("{}", i);
    }
    Ok(())
}
