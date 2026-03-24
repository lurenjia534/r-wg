use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{uniform_list, InteractiveElement as _, StatefulInteractiveElement as _, *};
use gpui_component::{
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    resizable::{h_resizable, resizable_panel, ResizableState},
    scroll::{ScrollableElement, Scrollbar},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, PixelsExt, Sizable as _,
    StyledExt as _,
};

use super::super::format::{format_addresses, format_allowed_ips, format_dns, format_route_table};
use super::super::state::{
    ConfigInspectorTab, ConfigSource, ConfigsLibraryRow, ConfigsPrimaryPane, ConfigsWorkspace,
    DraftValidationState, EndpointFamily, WgApp,
};
use super::data::ConfigsViewData;

// API-preserving split: workspace bootstrap, responsive layouts, library,
// editor, and inspector rendering are separated without changing callers.
include!("configs/workspace.rs");
include!("configs/layout.rs");
include!("configs/library.rs");
include!("configs/editor.rs");
include!("configs/inspector.rs");
