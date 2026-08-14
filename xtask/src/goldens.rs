use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use eas_mail_protocol::wbxml::{Element, Node, decode, encode};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Scenario {
    request_xml: PathBuf,
    request_wbxml: PathBuf,
    response_xml: PathBuf,
    response_wbxml: PathBuf,
    #[serde(default)]
    opaque: Vec<OpaqueSidecar>,
}

#[derive(Debug, Deserialize)]
struct OpaqueSidecar {
    document: Document,
    marker: String,
    file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Document {
    Request,
    Response,
}

pub(super) fn run(root: &Path, accept: bool) -> Result<()> {
    let fixture_root = root.join("fixtures/eas");
    let mut manifests = fs::read_dir(&fixture_root)?
        .map(|entry| entry.map(|value| value.path().join("scenario.toml")))
        .collect::<std::io::Result<Vec<_>>>()?;
    manifests.retain(|path| path.is_file());
    manifests.sort();
    anyhow::ensure!(!manifests.is_empty(), "no EAS golden scenarios found");
    for manifest in manifests {
        verify_scenario(&manifest, accept)?;
    }
    Ok(())
}

fn verify_scenario(manifest_path: &Path, accept: bool) -> Result<()> {
    let directory =
        manifest_path.parent().ok_or_else(|| anyhow::anyhow!("scenario has no directory"))?;
    let scenario: Scenario = toml::from_str(&fs::read_to_string(manifest_path)?)?;
    verify_document(
        directory,
        &scenario,
        Document::Request,
        &scenario.request_xml,
        &scenario.request_wbxml,
        accept,
    )?;
    verify_document(
        directory,
        &scenario,
        Document::Response,
        &scenario.response_xml,
        &scenario.response_wbxml,
        accept,
    )
}

fn verify_document(
    directory: &Path,
    scenario: &Scenario,
    document: Document,
    xml_path: &Path,
    wbxml_path: &Path,
    accept: bool,
) -> Result<()> {
    let xml_path = directory.join(xml_path);
    let wbxml_path = directory.join(wbxml_path);
    let mut element = parse_xml(&fs::read_to_string(&xml_path)?)
        .with_context(|| format!("cannot parse {}", xml_path.display()))?;
    for sidecar in scenario.opaque.iter().filter(|value| value.document == document) {
        let payload = fs::read(directory.join(&sidecar.file))?;
        anyhow::ensure!(
            replace_marker(&mut element, &sidecar.marker, &payload),
            "opaque marker {} is missing in {}",
            sidecar.marker,
            xml_path.display()
        );
    }
    let encoded = encode(&element)?;
    if accept {
        fs::write(&wbxml_path, &encoded)?;
    } else {
        let expected = fs::read(&wbxml_path).with_context(|| {
            format!("missing golden {}; run goldens accept", wbxml_path.display())
        })?;
        anyhow::ensure!(encoded == expected, "byte mismatch for {}", wbxml_path.display());
    }
    let decoded = decode(&encoded)?.ok_or_else(|| anyhow::anyhow!("golden decoded as empty"))?;
    anyhow::ensure!(decoded == element, "semantic mismatch for {}", wbxml_path.display());
    Ok(())
}

fn parse_xml(input: &str) -> Result<Element> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event()? {
            Event::Start(start) => {
                let inherited = stack.last().map(|value: &Element| value.namespace.as_str());
                stack.push(start_element(&reader, &start, inherited)?);
            }
            Event::Empty(start) => {
                let inherited = stack.last().map(|value: &Element| value.namespace.as_str());
                let element = start_element(&reader, &start, inherited)?;
                append_element(&mut stack, &mut root, element)?;
            }
            Event::Text(text) => {
                let value = text.xml10_content()?.into_owned();
                if !value.trim().is_empty() {
                    let parent = stack
                        .last_mut()
                        .ok_or_else(|| anyhow::anyhow!("text outside the XML root"))?;
                    parent.content.push(Node::Text(value));
                }
            }
            Event::End(_) => {
                let element = stack.pop().ok_or_else(|| anyhow::anyhow!("unbalanced XML"))?;
                append_element(&mut stack, &mut root, element)?;
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) => {}
            _ => return Err(anyhow::anyhow!("unsupported canonical XML event")),
        }
    }
    anyhow::ensure!(stack.is_empty(), "unclosed XML element");
    root.ok_or_else(|| anyhow::anyhow!("canonical XML is empty"))
}

fn start_element(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    inherited: Option<&str>,
) -> Result<Element> {
    let name = std::str::from_utf8(start.local_name().as_ref())?.to_owned();
    let mut namespace = inherited.map(str::to_owned);
    for attribute in start.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == b"xmlns" {
            namespace = Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())?
                    .into_owned(),
            );
        } else {
            return Err(anyhow::anyhow!("canonical XML attributes other than xmlns are forbidden"));
        }
    }
    let namespace = namespace.ok_or_else(|| anyhow::anyhow!("element has no EAS namespace"))?;
    Ok(Element::new(namespace, name))
}

fn append_element(
    stack: &mut [Element],
    root: &mut Option<Element>,
    element: Element,
) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.push(element);
    } else if root.replace(element).is_some() {
        return Err(anyhow::anyhow!("canonical XML has multiple roots"));
    }
    Ok(())
}

fn replace_marker(element: &mut Element, marker: &str, payload: &[u8]) -> bool {
    let mut replaced = false;
    for node in &mut element.content {
        match node {
            Node::Text(value) if value == marker => {
                *node = Node::Opaque(payload.to_vec());
                replaced = true;
            }
            Node::Element(child) => replaced |= replace_marker(child, marker, payload),
            Node::Text(_) | Node::Opaque(_) => {}
        }
    }
    replaced
}
