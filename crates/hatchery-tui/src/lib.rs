pub mod hq_edge;
#[cfg(feature = "arcade")]
pub mod app;
#[cfg(feature = "arcade")]
pub mod client;
#[cfg(feature = "arcade")]
pub mod control_plane;
#[cfg(feature = "arcade")]
pub mod diagnostics;
#[cfg(feature = "arcade")]
pub(crate) mod frame_capture;
#[cfg(feature = "arcade")]
pub mod icons;
#[cfg(feature = "arcade")]
pub mod pty_palette;
#[cfg(feature = "arcade")]
pub mod platform;
#[cfg(feature = "arcade")]
pub(crate) mod pet_arcade;
#[cfg(feature = "arcade")]
pub(crate) mod png_encode;
#[cfg(feature = "arcade")]
pub mod preferences;
#[cfg(feature = "arcade")]
pub(crate) mod profile;
#[cfg(feature = "arcade")]
pub(crate) mod pty_render_cache;
#[cfg(feature = "arcade")]
pub mod render;
#[cfg(feature = "arcade")]
pub(crate) mod shimmer;
#[cfg(feature = "arcade")]
pub mod surface;
#[cfg(feature = "arcade")]
pub mod terminal_bg;
#[cfg(feature = "arcade")]
pub mod text_editor;

#[cfg(feature = "arcade")]
pub use app::{
    AddSpaceDialog, AddSpaceField, App, AppAction, ConnectionState, ControlSection, Focus,
    GitLocationDialogKind, HistoryDialog, LaunchContextMode, LaunchField, LaunchTarget, LayoutRects,
    LoadedHistoryView,
    ManagedSessionView, MenuPlacement, NodeView, Provider, PtyColorMode, RosterMode,
    SessionAddress, SessionView, SidebarMode, SidebarPresentation, SpawnDialog, UiKey, WorkspaceView,
};
#[cfg(feature = "arcade")]
pub use client::{run, HarnessOperatorEndpoint, RunOptions};
#[cfg(feature = "arcade")]
pub use preferences::UiPreferences;
#[cfg(feature = "arcade")]
pub use render::render;
#[cfg(feature = "arcade")]
pub use surface::{LayoutPreset, Pane, PaneId, PaneNode, SplitAxis, SurfaceDropZone, SurfaceState};
#[cfg(feature = "arcade")]
pub use text_editor::{
    CursorPosition as TextCursorPosition, SyncState as TextSyncState, SyntaxClass,
    SyntaxLanguage, SyntaxSpan, TextEditor,
};
