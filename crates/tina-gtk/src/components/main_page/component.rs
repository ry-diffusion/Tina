// In-app page: thin shell wiring `Sidebar` and `ChatArea` onto an
// `AdwOverlaySplitView`. The dispatch logic lives in `dispatch.rs`.

use adw::prelude::*;
use relm4::prelude::*;

use crate::components::chat_area::{ChatArea, ChatAreaInit};
use crate::components::sidebar::{Sidebar, SidebarInit};

use super::messages::{MainInit, MainInput, MainOutput};
use super::model::MainPage;

#[relm4::component(pub)]
impl SimpleComponent for MainPage {
    type Init = MainInit;
    type Input = MainInput;
    type Output = MainOutput;

    view! {
        #[root]
        adw::BreakpointBin {
            set_width_request: 360,
            set_height_request: 200,

            #[wrap(Some)]
            #[name(split_view)]
            set_child = &adw::OverlaySplitView {
                set_min_sidebar_width: 280.0,
                set_max_sidebar_width: 380.0,
                set_sidebar_width_fraction: 0.27,

                #[wrap(Some)]
                set_sidebar = model.sidebar.widget(),

                #[wrap(Some)]
                set_content = model.chat_area.widget(),
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sidebar = Sidebar::builder()
            .launch(SidebarInit {
                avatars: init.avatars.clone(),
            })
            .forward(sender.input_sender(), MainInput::FromSidebar);

        let chat_area = ChatArea::builder()
            .launch(ChatAreaInit {
                avatars: init.avatars.clone(),
                media: init.media.clone(),
            })
            .forward(sender.input_sender(), MainInput::FromChatArea);

        let model = MainPage {
            service: init.service,
            sidebar,
            chat_area,
            split_view: adw::OverlaySplitView::new(),
        };

        let widgets = view_output!();
        // Replace the placeholder split_view in the model with the one
        // the macro just built so external callers (and the chat-area
        // toggle handler) operate on the live widget.
        let model = MainPage {
            split_view: widgets.split_view.clone(),
            ..model
        };

        // Adaptive collapse: below ~600sp the sidebar overlays the
        // content instead of sharing the row.
        let bp = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 600sp")
                .expect("hardcoded breakpoint condition is well-formed"),
        );
        bp.add_setter(&widgets.split_view, "collapsed", Some(&true.to_value()));
        root.add_breakpoint(bp);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: MainInput, sender: ComponentSender<Self>) {
        self.dispatch(msg, sender);
    }
}
