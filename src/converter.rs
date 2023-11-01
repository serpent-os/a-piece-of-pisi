// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! Convert input package to a yaml file

use std::{path::PathBuf, vec};

use thiserror::Error;
use url::Url;

use crate::eopkg::index::Package;

pub struct HashedPackage {
    /// Finalised hash
    pub hash: [u8; 32],

    /// Actual package itself
    pub package: Package,
}

/// For the given input packages, yield a functioning
/// boulder recipe as a string
pub fn convert(input: Vec<&HashedPackage>, base_uri: Url) -> Result<String, Error> {
    let mut upstreams = vec![];
    for pkg in input.iter() {
        let uri = base_uri.join(&pkg.package.package_uri)?.to_string();
        upstreams.push(format!(
            " - {}:\n    unpack: false\n    hash: {}",
            uri,
            const_hex::encode(pkg.hash)
        ));
    }

    let sample = &input.first().ok_or(Error::NoPackage)?;
    let homepage = sample
        .package
        .source
        .homepage
        .clone()
        .unwrap_or("no-homepage-set".into());
    let licenses = sample.package.licenses.iter().map(|l| format!("    - {l}"));
    let yml = vec![
        format!("name: {}", sample.package.source.name),
        format!("version: \"{}\"", sample.package.history.updates[0].version),
        format!("release: {}", sample.package.history.updates[0].release),
        format!("homepage: {}", homepage),
        "upstreams:".into(),
        upstreams.join("\n"),
        format!("summary: {}", sample.package.summary),
        format!(
            "description: |\n    {}",
            sample.package.description.replace('\n', " ")
        ),
        "strip: false".into(),
        "license: ".into(),
        licenses.collect::<Vec<String>>().join("\n"),
        "install:  |".into(),
        generate_install_script(&input, &base_uri)?,
    ];

    Ok(yml.join("\n"))
}

fn generate_install_script(input: &[&HashedPackage], base_uri: &Url) -> Result<String, Error> {
    let mut zips = vec![];
    for pkg in input.iter() {
        let url = base_uri.join(&pkg.package.package_uri)?;
        let path = PathBuf::from(url.path());
        let name = path.file_name().ok_or(Error::Path)?.to_string_lossy();
        zips.push(format!("    unzip -o %(sourcedir)/{name}"));
        zips.push("    tar xf install.tar.xz -C %(installroot)".to_string());
    }

    Ok(format!(
        "    %install_dir %(installroot)\n{}",
        zips.join("\n")
    ))
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("path issue")]
    Path,

    #[error("no package")]
    NoPackage,

    #[error("url: {0}")]
    Url(#[from] url::ParseError),
}
