/// One WBXML content node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A nested XML element.
    Element(Element),
    /// Inline UTF-8 text.
    Text(String),
    /// Binary OPAQUE data.
    Opaque(Vec<u8>),
}

/// Namespace-qualified WBXML element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// EAS namespace, for example `AirSync`.
    pub namespace: String,
    /// Local element name.
    pub name: String,
    /// Ordered mixed content.
    pub content: Vec<Node>,
}

impl Element {
    /// Creates an empty element.
    #[must_use]
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self { namespace: namespace.into(), name: name.into(), content: Vec::new() }
    }

    /// Creates an element with one text node.
    #[must_use]
    pub fn text(
        namespace: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let mut element = Self::new(namespace, name);
        element.content.push(Node::Text(value.into()));
        element
    }

    /// Appends a child element.
    pub fn push(&mut self, child: Element) {
        self.content.push(Node::Element(child));
    }

    /// Returns the first direct child with the requested name and namespace.
    #[must_use]
    pub fn child(&self, namespace: &str, name: &str) -> Option<&Element> {
        self.content.iter().find_map(|node| match node {
            Node::Element(child)
                if child.namespace.eq_ignore_ascii_case(namespace) && child.name == name =>
            {
                Some(child)
            }
            _ => None,
        })
    }

    /// Returns the first matching element at any depth.
    #[must_use]
    pub fn descendant(&self, namespace: &str, name: &str) -> Option<&Element> {
        if self.namespace.eq_ignore_ascii_case(namespace) && self.name == name {
            return Some(self);
        }
        self.children().find_map(|child| child.descendant(namespace, name))
    }

    /// Collects matching elements at any depth in document order.
    #[must_use]
    pub fn descendants(&self, namespace: &str, name: &str) -> Vec<&Element> {
        let mut output = Vec::new();
        self.collect_descendants(namespace, name, &mut output);
        output
    }

    fn collect_descendants<'a>(
        &'a self,
        namespace: &str,
        name: &str,
        output: &mut Vec<&'a Element>,
    ) {
        if self.namespace.eq_ignore_ascii_case(namespace) && self.name == name {
            output.push(self);
        }
        for child in self.children() {
            child.collect_descendants(namespace, name, output);
        }
    }

    /// Iterates direct child elements.
    pub fn children(&self) -> impl Iterator<Item = &Element> {
        self.content.iter().filter_map(|node| match node {
            Node::Element(child) => Some(child),
            Node::Text(_) | Node::Opaque(_) => None,
        })
    }

    /// Returns concatenated textual content. OPAQUE bytes are not coerced to text.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|node| match node {
                Node::Text(value) => Some(value.as_str()),
                Node::Element(_) | Node::Opaque(_) => None,
            })
            .collect()
    }

    /// Returns the first OPAQUE payload directly contained by this element.
    #[must_use]
    pub fn opaque_content(&self) -> Option<&[u8]> {
        self.content.iter().find_map(|node| match node {
            Node::Opaque(value) => Some(value.as_slice()),
            Node::Element(_) | Node::Text(_) => None,
        })
    }
}
