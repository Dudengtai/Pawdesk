//! System tray (prd F-TR-01/02, TRAY-01..03).

use std::path::Path;

use tracing::{info, warn};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::error::AppError;
use crate::event::TrayCommand;

pub struct TrayHandle {
    _tray: TrayIcon,
    pub exit_id: tray_icon::menu::MenuId,
    pub show_id: tray_icon::menu::MenuId,
    pub hide_id: tray_icon::menu::MenuId,
    pub pause_id: tray_icon::menu::MenuId,
    pub settings_id: tray_icon::menu::MenuId,
}

impl TrayHandle {
    pub fn new(icon_path: &Path) -> Result<Self, AppError> {
        let icon = load_icon(icon_path)?;

        let show = MenuItem::new("显示宠物", true, None);
        let hide = MenuItem::new("隐藏宠物", true, None);
        let pause = MenuItem::new("暂停提醒", true, None);
        let settings = MenuItem::new("打开设置", true, None);
        let exit = MenuItem::new("退出", true, None);

        let show_id = show.id().clone();
        let hide_id = hide.id().clone();
        let pause_id = pause.id().clone();
        let settings_id = settings.id().clone();
        let exit_id = exit.id().clone();

        let menu = Menu::new();
        menu.append(&show)
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&hide)
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&pause)
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&settings)
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;
        menu.append(&exit)
            .map_err(|e| AppError::Platform(format!("tray menu append: {e}")))?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("PawDesk — 桌面互动宠物")
            .with_icon(icon)
            .build()
            .map_err(|e| AppError::Platform(format!("create tray icon failed: {e}")))?;

        info!("system tray created");

        Ok(Self {
            _tray: tray,
            exit_id,
            show_id,
            hide_id,
            pause_id,
            settings_id,
        })
    }

    pub fn poll_command(&self) -> Option<TrayCommand> {
        let receiver = MenuEvent::receiver();
        match receiver.try_recv() {
            Ok(event) => {
                if event.id == self.exit_id {
                    Some(TrayCommand::Exit)
                } else if event.id == self.show_id {
                    Some(TrayCommand::ShowPet)
                } else if event.id == self.hide_id {
                    Some(TrayCommand::HidePet)
                } else if event.id == self.pause_id {
                    Some(TrayCommand::ToggleReminderPause)
                } else if event.id == self.settings_id {
                    Some(TrayCommand::OpenSettings)
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }

    /// Update tooltip to reflect reminder state (tray menu text APIs vary).
    pub fn set_tooltip(&mut self, text: &str) {
        if let Err(e) = self._tray.set_tooltip(Some(text)) {
            warn!(error = %e, "tray set_tooltip failed");
        }
    }
}

fn load_icon(path: &Path) -> Result<Icon, AppError> {
    let img = image::open(path)
        .map_err(|e| AppError::Asset(format!("load tray icon {}: {e}", path.display())))?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Icon::from_rgba(img.into_raw(), width, height)
        .map_err(|e| AppError::Asset(format!("create tray Icon: {e}")))
}


