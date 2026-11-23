//! graph-tui: a general-purpose graph rendering engine.
//!
//! Input is a graph (nodes + edges + edge groups); the output is a picture of
//! character cells in a terminal: edges are orthogonal polylines, corners use
//! rounded box-drawing glyphs, the four stroke kinds `- | / \` are merged into a
//! glyph where they meet, of two crossing edges one passes under the other (breaking
//! one cell), and each cell holds a single colour.
//!
//! Layers:
//!   document native document model (box / text / edge separate, coordinates are cells)
//!   model    intermediate structure for .dot / .canvas (import/export)
//!   dot      .dot subset parser; dot_write writes it back (save after editing)
//!   scene    canvas coordinates (one graph can have several coordinate sets)
//!   layout   layout algorithms: geo / relax / force / layered
//!   canvas   rasterisation: draw coordinates into direction bits, then merge glyphs
//!   render   projection, zoom/pan, ports and labels, legend
//!   text      inline styles in labels (**bold** / /italic/)
//!   svg       one frame -> self-contained SVG; json/canvas_io  JSON Canvas import/export
//!   md       minimal Markdown -> terminal text (the `--help` section of docs/usage.md)
//!   keys     key bindings and preferences (config.ini)
//!   app      interaction state and events
//!   search   search
pub mod app;
pub mod canvas;
pub mod canvas_io;
pub mod convert;
pub mod document;
pub mod document_io;
pub mod dot;
pub mod dot_write;
pub mod dump;
pub mod engine;
pub mod external;
pub mod glyphs;
pub mod headless;
pub mod help;
pub mod json;
pub mod keys;
pub mod lang;
pub mod layout;
pub mod link;
pub mod md;
pub mod model;
pub mod paths;
pub mod profile;
pub mod render;
pub mod scene;
pub mod search;
pub mod svg;
pub mod target;
pub mod text;
pub mod tips;
pub mod tutor;

pub use lang::Lang;
pub use layout::Layout;
pub use model::{Builder, Edge, Graph, Group, Node};
