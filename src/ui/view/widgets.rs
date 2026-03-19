use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, tag::Tag, v_flex, ActiveTheme as _, Sizable as _, StyledExt as _};

use super::super::state::{BackendDiagnostic, BackendHealth};

pub(crate) fn backend_status_badge(diagnostic: &BackendDiagnostic) -> Tag {
    backend_status_tag(diagnostic, diagnostic.badge_label())
}

pub(crate) fn backend_status_tag(
    diagnostic: &BackendDiagnostic,
    label: impl Into<SharedString>,
) -> Tag {
    let label = label.into();
    match diagnostic.health {
        BackendHealth::Running => Tag::success().small().rounded_full().child(label),
        BackendHealth::Checking | BackendHealth::Working { .. } => {
            Tag::info().small().rounded_full().child(label)
        }
        BackendHealth::AccessDenied
        | BackendHealth::VersionMismatch { .. }
        | BackendHealth::Unreachable => Tag::danger().small().rounded_full().child(label),
        BackendHealth::Installed | BackendHealth::NotInstalled => {
            Tag::warning().small().rounded_full().child(label)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        BackendHealth::Unsupported => Tag::secondary().small().rounded_full().child(label),
        BackendHealth::Unknown => Tag::secondary().small().rounded_full().child(label),
    }
}

pub(crate) struct PageShellHeader {
    eyebrow: SharedString,
    title: SharedString,
    subtitle: SharedString,
    actions: Option<AnyElement>,
}

impl PageShellHeader {
    pub(crate) fn new(
        eyebrow: impl Into<SharedString>,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
    ) -> Self {
        Self {
            eyebrow: eyebrow.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            actions: None,
        }
    }

    pub(crate) fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }
}

enum PageShellHeaderKind {
    Standard(PageShellHeader),
    Custom(AnyElement),
}

pub(crate) struct PageShell {
    header: PageShellHeaderKind,
    toolbar: Option<AnyElement>,
    body: AnyElement,
}

impl PageShell {
    pub(crate) fn new(header: PageShellHeader, body: impl IntoElement) -> Self {
        Self {
            header: PageShellHeaderKind::Standard(header),
            toolbar: None,
            body: body.into_any_element(),
        }
    }

    pub(crate) fn custom_header(header: impl IntoElement, body: impl IntoElement) -> Self {
        Self {
            header: PageShellHeaderKind::Custom(header.into_any_element()),
            toolbar: None,
            body: body.into_any_element(),
        }
    }

    pub(crate) fn toolbar(mut self, toolbar: impl IntoElement) -> Self {
        self.toolbar = Some(toolbar.into_any_element());
        self
    }

    pub(crate) fn render<T>(self, cx: &mut Context<T>) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tiles)
            .overflow_hidden()
            .child(match self.header {
                PageShellHeaderKind::Standard(header) => {
                    render_page_shell_header(header, cx).into_any_element()
                }
                PageShellHeaderKind::Custom(header) => header,
            })
            .when_some(self.toolbar, |this, toolbar| this.child(toolbar))
            .child(self.body)
    }
}

fn render_page_shell_header<T>(header: PageShellHeader, cx: &mut Context<T>) -> Div {
    div()
        .px_5()
        .py_4()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .flex_wrap()
                .gap_4()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child(header.eyebrow),
                        )
                        .child(div().text_xl().font_semibold().child(header.title))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(header.subtitle),
                        ),
                )
                .when_some(header.actions, |this, actions| this.child(actions)),
        )
}
