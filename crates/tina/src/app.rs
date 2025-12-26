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
    AppCoreInitializing,
    Welcome,
}

impl Tina {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Ready => {
                tracing::info!("Tina is ready!");
                self.scene = Scene::Welcome;
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

    pub fn theme(&self) -> Option<iced::Theme> {
        Some(iced::Theme::Dark)
    }
}
