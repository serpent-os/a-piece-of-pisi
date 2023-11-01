// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

use std::{
    collections::{BTreeMap, HashMap},
    fs::{create_dir, remove_dir_all, File},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use a_piece_of_pisi::{
    converter::{convert, HashedPackage},
    eopkg::{
        self,
        index::{Index, Package},
    },
};
use crossterm::style::Stylize;
use dag::Dag;
use indicatif::{style::TemplateError, MultiProgress, ProgressBar, ProgressStyle};
use lzma::LzmaReader;
use reqwest::Url;
use serde_xml_rs::from_reader;

use futures::{stream, StreamExt, TryStreamExt};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::ParseError;

use color_eyre::Result;

/// Limit concurrency to 8 jobs
const CONCURRENCY_LIMIT: usize = 8;

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

    #[error("unknown package")]
    UnknownPackage,
}

/// Asynchronously fetch a package
/// TODO: Filter already fetched!
async fn fetch(
    multi: &MultiProgress,
    total: &ProgressBar,
    p: &Package,
    origin: &Url,
    cache_dir: &Path,
) -> Result<HashedPackage, Error> {
    let uri = origin.join(&p.package_uri)?;
    let path = uri
        .path_segments()
        .ok_or(Error::InvalidURI)?
        .last()
        .ok_or(Error::InvalidURI)?
        .to_string();
    let mut r = reqwest::get(uri).await?;
    let pbar = multi.insert_before(total, ProgressBar::new(p.package_size));
    pbar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}]  {bar:20.cyan/blue}  {bytes:>7}/{total_bytes:7} {wide_msg:>.dim}",
        )?
        .progress_chars("##-"),
    );
    pbar.set_message(path.clone());
    pbar.enable_steady_tick(Duration::from_millis(150));

    let mut hasher = Sha256::new();
    let output_path = cache_dir.join(&path);
    let mut output = File::create(&output_path).unwrap();

    while let Some(chunk) = &r.chunk().await? {
        let mut cursor = Cursor::new(chunk);
        let len = chunk.len();
        std::io::copy(&mut cursor, &mut output)?;
        pbar.inc(len as u64);
        hasher.update(chunk);
    }
    let hash = hasher.finalize();

    pbar.println(format!("{} {}", "Fetched".green(), path.clone().bold()));
    total.inc(1);

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
    let origin = Url::parse("https://packages.getsol.us/unstable/")?;
    let cache_dir = PathBuf::from("cache");
    if !cache_dir.exists() {
        create_dir(&cache_dir)?;
    }

    let mapping: BTreeMap<_, _> = index.packages.iter().map(|p| (p.name.clone(), p)).collect();

    let mut base = mapping
        .values()
        .filter_map(|m| {
            if let Some(component) = &m.part_of {
                if component == "system.base" || component == "system.devel" {
                    Some(m.name.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let extensions = [
        "libgcrypt",
        "libgnutls",
        "lsb-release",
        "inxi",
        "file",
        "tree",
        "which",
        "man-db",
    ];
    base.extend(extensions.iter().map(|m| m.to_string()));

    let mut graph: Dag<String> = Dag::new();

    // Solve ...
    let mut processing = base.clone();
    while !&processing.is_empty() {
        let mut next = vec![];
        for pkg in processing.iter() {
            let pkg = mapping.get(pkg).ok_or(Error::UnknownPackage)?;
            let our_index = graph.add_node_or_get_index(pkg.name.clone());
            if let Some(deps) = &pkg.run_deps {
                for dep in &deps.deps {
                    let child_index = if let Some(child_index) = graph.get_index(&dep.value) {
                        // Already exists..
                        child_index
                    } else {
                        // Create the child index.
                        next.push(dep.value.clone());
                        graph.add_node_or_get_index(dep.value.clone())
                    };
                    graph.add_edge(our_index, child_index);
                }
            }
        }
        processing = next;
    }

    // Fetch within the dependency set
    let packages = graph.topo().cloned().collect::<Vec<_>>();

    let total_progress = multi.add(
        ProgressBar::new(packages.len() as u64).with_style(
            ProgressStyle::with_template("\n|{bar:20.cyan/blue}| {pos}/{len}")
                .unwrap()
                .progress_chars("##-"),
        ),
    );
    total_progress.tick();

    let packages = packages.iter().filter_map(|p| mapping.get(p));
    let results: Vec<HashedPackage> = stream::iter(
        packages.map(|f| async { fetch(&multi, &total_progress, f, &origin, &cache_dir).await }),
    )
    .buffer_unordered(CONCURRENCY_LIMIT)
    .try_collect()
    .await?;

    // Convert to a hashmap
    let mut source_buckets: HashMap<String, Vec<&HashedPackage>> = HashMap::new();
    for result in results.iter() {
        let source_name = result.package.source.name.clone();
        if let Some(bucket) = source_buckets.get_mut(&source_name) {
            bucket.push(result)
        } else {
            source_buckets.insert(source_name, vec![result]);
        };
    }

    let base_dir = PathBuf::from("binary-conversion");
    if base_dir.exists() {
        remove_dir_all(&base_dir)?;
    }
    create_dir(&base_dir)?;

    // Conversion time.
    for (source, packages) in total_progress.wrap_iter(source_buckets.iter()) {
        let tree = base_dir.join(source);
        create_dir(&tree)?;
        let yml_path = tree.join("stone.yml");
        let yml = convert(packages.clone(), origin.clone())?;
        let mut file = File::create(yml_path)?;
        file.write_all(yml.as_bytes())?;
    }
    Ok(())
}
