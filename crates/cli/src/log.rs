use {
    std::fmt::Error as FmtError,
    tracing_forest::{
        Formatter,
        printer::Pretty,
        tree::{Event, Tree},
    },
    smallvec::SmallVec,
};

pub struct PrettyCustom;

type IndentVec = SmallVec<[Indent; 32]>;

impl Formatter for PrettyCustom {
    type Error = FmtError;

    fn fmt(&self, tree: &Tree) -> Result<String, Self::Error> {}
}

impl PrettyCustom {
    fn format_tree(
        tree: &Tree,
        duration_root: Option<f64>,
        indent: &mut IndentVec,
        writer: &mut String,
    ) -> fmt::Result {
        match tree {
            Tree::Event(event) => {
                Pretty::format_shared(&event.shared, writer)?;
                Pretty::format_indent(indent, writer)?;
                Pretty::format_event(event, writer)
            }
            Tree::Span(span) => {
                Pretty::format_shared(&span.shared, writer)?;
                Pretty::format_indent(indent, writer)?;
                Pretty::format_span(span, duration_root, indent, writer)
            }
        }
    }
}

enum Indent {
    Null,
    Line,
    Fork,
    Turn,
}

impl Indent {
    fn repr(&self) -> &'static str {
        match self {
            Self::Null => "   ",
            Self::Line => "│  ",
            Self::Fork => "┝━ ",
            Self::Turn => "┕━ ",
        }
    }
}