// SPDX-FileCopyrightText: Copyright © 2020-2023 Serpent OS Developers
//
// SPDX-License-Identifier: MPL-2.0

//! eopkg index parsing

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct History {
    #[serde(rename = "Update")]
    pub updates: Vec<Update>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Dependency {
    #[serde(rename = "$value")]
    pub value: String,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RuntimeDependencies {
    #[serde(rename = "Dependency")]
    pub deps: Vec<Dependency>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Update {
    pub release: u64,
    #[serde(rename = "Date")]
    pub date: String,
    #[serde(rename = "Version")]
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Source {
    pub name: String,
    pub homepage: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Package {
    pub name: String,
    pub summary: String,
    pub description: String,
    pub part_of: Option<String>,
    #[serde(rename = "PackageURI")]
    pub package_uri: String,
    #[serde(rename = "PackageSize")]
    pub package_size: u64,
    pub package_hash: String,
    pub history: History,
    pub source: Source,
    #[serde(rename = "License")]
    pub licenses: Vec<String>,
    #[serde(rename = "RuntimeDependencies")]
    pub run_deps: Option<RuntimeDependencies>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Obsoletes {
    #[serde(rename = "$value")]
    pub packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Distro {
    pub source_name: String,
    pub version: String,
    pub r#type: String,
    pub obsoletes: Obsoletes,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "PISI", rename_all = "PascalCase")]
pub struct Index {
    pub distribution: Distro,
    #[serde(rename = "Package")]
    pub packages: Vec<Package>,
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use lzma::LzmaReader;
    use serde_xml_rs::from_reader;

    #[test]
    fn basic_index() {
        let reader = LzmaReader::new_decompressor(Cursor::new(include_bytes!(
            "../../test/eopkg-index.xml.xz"
        )))
        .unwrap();
        let doc: super::Index = from_reader(reader).unwrap();

        // Collect all *8* subpackages of zlib and itself
        let zlib = doc
            .packages
            .iter()
            .filter(|p| p.source.name == "zlib")
            .collect::<Vec<_>>();
        assert_eq!(zlib.len(), 6);
        let latest = &zlib[0].history.updates[0];
        assert_eq!(latest.version, "1.3");
        assert_eq!(latest.release, 26);

        let dep = zlib[0].run_deps.clone().expect("No dependencies");
        assert_eq!(dep.deps[0].value, "glibc");
    }
}
