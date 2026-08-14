use super::codepage::{code_page, code_page_for_namespace};
use super::{Element, Node};
use crate::{EasError, Result};

const SWITCH_PAGE: u8 = 0x00;
const END: u8 = 0x01;
const STR_I: u8 = 0x03;
const OPAQUE: u8 = 0xC3;
const MAX_DOCUMENT_BYTES: usize = 36 * 1024 * 1024;
const MAX_DEPTH: usize = 128;

/// Encodes an element tree as strict MS-ASWBXML.
pub fn encode(root: &Element) -> Result<Vec<u8>> {
    let mut output = vec![0x03, 0x01, 0x6A, 0x00];
    let mut current_page = 0;
    encode_element(root, &mut current_page, &mut output, 0)?;
    if output.len() > MAX_DOCUMENT_BYTES {
        return Err(EasError::Protocol("WBXML document exceeds 36 MiB".into()));
    }
    Ok(output)
}

/// Decodes strict MS-ASWBXML into an element tree.
pub fn decode(input: &[u8]) -> Result<Option<Element>> {
    if input.is_empty() {
        return Ok(None);
    }
    if input.len() > MAX_DOCUMENT_BYTES {
        return Err(EasError::Protocol("WBXML document exceeds 36 MiB".into()));
    }
    let mut reader = Reader::new(input);
    reader.read_header()?;
    reader.read_document()
}

fn encode_element(
    element: &Element,
    current_page: &mut u8,
    output: &mut Vec<u8>,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_DEPTH {
        return Err(EasError::Protocol("WBXML nesting exceeds 128 elements".into()));
    }
    let page = code_page_for_namespace(&element.namespace)?;
    if page.id != *current_page {
        output.extend_from_slice(&[SWITCH_PAGE, page.id]);
        *current_page = page.id;
    }
    let mut token = page.token(&element.name)?;
    if !element.content.is_empty() {
        token |= 0x40;
    }
    output.push(token);
    for node in &element.content {
        match node {
            Node::Element(child) => {
                encode_element(child, current_page, output, depth + 1)?;
            }
            Node::Text(text) => {
                if text.as_bytes().contains(&0) {
                    return Err(EasError::Protocol("inline text contains NUL".into()));
                }
                output.push(STR_I);
                output.extend_from_slice(text.as_bytes());
                output.push(0);
            }
            Node::Opaque(bytes) => {
                output.push(OPAQUE);
                encode_mbuint(bytes.len(), output);
                output.extend_from_slice(bytes);
            }
        }
    }
    if !element.content.is_empty() {
        output.push(END);
    }
    Ok(())
}

fn encode_mbuint(mut value: usize, output: &mut Vec<u8>) {
    let mut bytes = Vec::with_capacity(10);
    let maximum_bytes = (usize::BITS as usize).div_ceil(7);
    for _ in 0..maximum_bytes {
        bytes.push((value & 0x7F) as u8);
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    bytes.reverse();
    let length = bytes.len();
    for (index, byte) in bytes.iter_mut().enumerate() {
        if index + 1 != length {
            *byte |= 0x80;
        }
    }
    output.extend_from_slice(&bytes);
}

struct Reader<'a> {
    input: &'a [u8],
    cursor: usize,
    current_page: u8,
}

impl<'a> Reader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, cursor: 0, current_page: 0 }
    }

    fn read_header(&mut self) -> Result<()> {
        if self.byte()? != 0x03 || self.mbuint()? != 1 || self.mbuint()? != 0x6A {
            return Err(EasError::Protocol("unsupported WBXML header".into()));
        }
        if self.mbuint()? != 0 {
            return Err(EasError::Protocol("WBXML string tables are unsupported".into()));
        }
        Ok(())
    }

    fn read_document(&mut self) -> Result<Option<Element>> {
        let mut stack = Vec::new();
        let mut root = None;
        while self.cursor < self.input.len() {
            let token = self.byte()?;
            match token {
                SWITCH_PAGE => self.current_page = self.byte()?,
                END => finish_element(&mut stack, &mut root)?,
                STR_I => append_node(&mut stack, Node::Text(self.inline_string()?))?,
                OPAQUE => {
                    let length = self.mbuint()?;
                    let bytes = self.bytes(length)?.to_vec();
                    append_node(&mut stack, Node::Opaque(bytes))?;
                }
                0x02 | 0x04 | 0x40..=0x44 | 0x80..=0x84 | 0xC0..=0xC2 | 0xC4 => {
                    return Err(EasError::Protocol(format!(
                        "unsupported global WBXML token 0x{token:02X}"
                    )));
                }
                _ => self.start_element(token, &mut stack, &mut root)?,
            }
            if stack.len() > MAX_DEPTH {
                return Err(EasError::Protocol("WBXML nesting exceeds 128 elements".into()));
            }
        }
        if !stack.is_empty() {
            return Err(EasError::Protocol("WBXML document has unclosed elements".into()));
        }
        Ok(root)
    }

    fn start_element(
        &self,
        token: u8,
        stack: &mut Vec<Element>,
        root: &mut Option<Element>,
    ) -> Result<()> {
        if token & 0x80 != 0 {
            return Err(EasError::Protocol("WBXML attributes are unsupported".into()));
        }
        let page = code_page(self.current_page)?;
        let element = Element::new(page.namespace, page.tag(token & 0x3F)?);
        if token & 0x40 != 0 {
            stack.push(element);
        } else if let Some(parent) = stack.last_mut() {
            parent.push(element);
        } else if root.replace(element).is_some() {
            return Err(EasError::Protocol("WBXML document has multiple roots".into()));
        }
        Ok(())
    }

    fn byte(&mut self) -> Result<u8> {
        let value = self
            .input
            .get(self.cursor)
            .copied()
            .ok_or_else(|| EasError::Protocol("unexpected end of WBXML".into()))?;
        self.cursor += 1;
        Ok(value)
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(length)
            .filter(|end| *end <= self.input.len())
            .ok_or_else(|| EasError::Protocol("WBXML value exceeds document bounds".into()))?;
        let output = self
            .input
            .get(self.cursor..end)
            .ok_or_else(|| EasError::Protocol("invalid WBXML byte range".into()))?;
        self.cursor = end;
        Ok(output)
    }

    fn mbuint(&mut self) -> Result<usize> {
        let mut value = 0_usize;
        for _ in 0..10 {
            let byte = self.byte()?;
            value = value
                .checked_shl(7)
                .and_then(|head| head.checked_add(usize::from(byte & 0x7F)))
                .ok_or_else(|| EasError::Protocol("WBXML integer overflow".into()))?;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(EasError::Protocol("WBXML integer is too long".into()))
    }

    fn inline_string(&mut self) -> Result<String> {
        let remaining = self
            .input
            .get(self.cursor..)
            .ok_or_else(|| EasError::Protocol("invalid WBXML string offset".into()))?;
        let length = remaining
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| EasError::Protocol("unterminated WBXML inline string".into()))?;
        let bytes = self.bytes(length)?;
        self.cursor += 1;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| EasError::Protocol("WBXML inline string is not UTF-8".into()))
    }
}

fn append_node(stack: &mut [Element], node: Node) -> Result<()> {
    let parent = stack
        .last_mut()
        .ok_or_else(|| EasError::Protocol("WBXML content appears outside an element".into()))?;
    parent.content.push(node);
    Ok(())
}

fn finish_element(stack: &mut Vec<Element>, root: &mut Option<Element>) -> Result<()> {
    let element = stack
        .pop()
        .ok_or_else(|| EasError::Protocol("WBXML END appears out of sequence".into()))?;
    if let Some(parent) = stack.last_mut() {
        parent.push(element);
    } else if root.replace(element).is_some() {
        return Err(EasError::Protocol("WBXML document has multiple roots".into()));
    }
    Ok(())
}
