use iced::{Element, widget::*};

#[derive(Default, Debug)]
pub struct Tina {
    scene: Scene,
}

#[derive(Debug, Clone)]
pub enum Message {
    Ready,
    InitError { reason: String, details: String },
}

#[derive(Default, Debug, Clone)]
pub enum Scene {
    #[default]
    /** Downloading the nanachi dependencies and initializing it. It will be slow on the first startup */
    AppCoreInitializing,

    /** Welcome screen */
    Welcome,
}

impl Tina {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Ready => {
                tracing::info!("Tina is ready!");
            }
            Message::InitError { reason, details } => {
                tracing::error!("Initialization Error: {}: {}", reason, details);
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.scene {
            Scene::AppCoreInitializing => center(text("App is initializing..")).into(),
            Scene::Welcome => center(text("Welcome to Tina!")).into(),
        }
    }

    pub fn theme(&self) -> Option<Theme> {
        Some(iced::Theme::Dark)
    }
}
