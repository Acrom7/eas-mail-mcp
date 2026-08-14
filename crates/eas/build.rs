use eas_mail_profile::{VerifiedBundle, load};
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct PageSpec {
    page: u8,
    namespace: String,
    xmlns: String,
    #[serde(default)]
    tags: Vec<TagSpec>,
}

#[derive(Deserialize)]
struct TagSpec {
    token: u8,
    name: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(required_environment("CARGO_MANIFEST_DIR")?);
    let workspace = manifest.join("../..");
    let spec_dir = manifest.join("../../spec/codepages");
    println!("cargo:rerun-if-changed={}", spec_dir.display());
    println!("cargo:rerun-if-env-changed=EAS_MAIL_PROFILE_BUNDLE");
    generate_profiles(&workspace)?;
    let mut paths = read_paths(&spec_dir)?;
    paths.sort();

    let mut pages = paths.iter().map(|path| read_page(path)).collect::<Result<Vec<_>, _>>()?;
    pages.sort_by_key(|page| page.page);
    validate(&pages)?;

    let mut generated = String::from("pub static CODE_PAGES: &[CodePage] = &[\n");
    for page in pages {
        writeln!(
            generated,
            "    CodePage {{ id: {}, namespace: {:?}, xmlns: {:?}, tags: &[",
            page.page, page.namespace, page.xmlns
        )?;
        for tag in page.tags {
            writeln!(generated, "        ({}, {:?}),", tag.token, tag.name)?;
        }
        generated.push_str("    ] },\n");
    }
    generated.push_str("];\n");

    let output = PathBuf::from(required_environment("OUT_DIR")?).join("codepages.rs");
    fs::write(output, generated)?;
    Ok(())
}

fn read_paths(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .collect())
}

fn read_page(path: &Path) -> Result<PageSpec, Box<dyn Error>> {
    let input = fs::read_to_string(path)?;
    toml::from_str(&input).map_err(Into::into)
}

fn validate(pages: &[PageSpec]) -> io::Result<()> {
    if pages.len() != 25 {
        return Err(io::Error::other("all 25 MS-ASWBXML code pages are required"));
    }
    for (expected, page) in pages.iter().enumerate() {
        if usize::from(page.page) != expected {
            return Err(io::Error::other("code pages must be contiguous"));
        }
        let mut tokens = page.tags.iter().map(|tag| tag.token).collect::<Vec<_>>();
        tokens.sort_unstable();
        tokens.dedup();
        if tokens.len() != page.tags.len() {
            return Err(io::Error::other(format!("duplicate token on page {}", page.page)));
        }
        let mut names = page.tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        if names.len() != page.tags.len() {
            return Err(io::Error::other(format!("duplicate tag on page {}", page.page)));
        }
    }
    Ok(())
}

fn required_environment(name: &str) -> io::Result<std::ffi::OsString> {
    env::var_os(name).ok_or_else(|| io::Error::other(format!("{name} is not set")))
}

fn generate_profiles(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let configured = env::var_os("EAS_MAIL_PROFILE_BUNDLE").map(PathBuf::from);
    let source = match configured {
        Some(path) if path.is_absolute() => path,
        Some(path) => workspace.join(path),
        None => workspace.join("profile.example.toml"),
    };
    let bundle = load(&source)?;
    println!("cargo:rerun-if-changed={}", bundle.source.display());
    for profile in &bundle.profiles {
        if let Some(path) = &profile.pem_source {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let output = PathBuf::from(required_environment("OUT_DIR")?);
    let generated = render_profiles(&bundle, &output)?;
    fs::write(output.join("profiles.rs"), generated)?;
    Ok(())
}

fn render_profiles(bundle: &VerifiedBundle, output: &Path) -> Result<String, Box<dyn Error>> {
    let mut generated = String::new();
    writeln!(generated, "static COMPILED_PROFILES: &[Profile] = &[")?;
    for (index, verified) in bundle.profiles.iter().enumerate() {
        let profile = &verified.spec;
        writeln!(generated, "    Profile {{")?;
        writeln!(generated, "        key: {:?},", profile.id)?;
        writeln!(generated, "        display_name: {:?},", profile.display_name)?;
        writeln!(generated, "        host: {:?},", profile.host)?;
        writeln!(generated, "        email_domains: &{:?},", profile.email_domains)?;
        writeln!(generated, "        username_realm: {:?},", profile.username_realm)?;
        writeln!(generated, "        device_id_length: {},", profile.device_id_length)?;
        if let Some(pem) = &verified.pem {
            let name = format!("profile-ca-{index}.pem");
            fs::write(output.join(&name), pem)?;
            writeln!(
                generated,
                "        extra_ca_pem: Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}\"))),"
            )?;
        } else {
            writeln!(generated, "        extra_ca_pem: None,")?;
        }
        writeln!(generated, "    }},")?;
    }
    writeln!(generated, "];")?;
    writeln!(generated, "static COMPILED_REGISTRY: ProfileRegistry = ProfileRegistry {{")?;
    writeln!(generated, "    bundle_version: {:?},", bundle.manifest.bundle_version)?;
    writeln!(generated, "    bundle_hash: {:?},", bundle.hash)?;
    writeln!(generated, "    development_only: {},", bundle.manifest.development_only)?;
    writeln!(generated, "    profiles: COMPILED_PROFILES,")?;
    writeln!(generated, "}};")?;
    Ok(generated)
}
