use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

macro_rules! embedded_assets {
    ($($path:literal),+ $(,)?) => {
        const FILES: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!("../assets/", $path)))),+
        ];
    };
}

embedded_assets!(
    "icon/menu.svg",
    "brand/wordmark.svg",
    "icon/projects.svg",
    "icon/packages.svg",
    "icon/templates.svg",
    "icon/logs.svg",
    "icon/settings.svg",
    "icon/copy-small.svg",
    "icon/star.svg",
    "icon/avatar.svg",
    "icon/more-vertical.svg",
    "icon/remove.svg",
    "icon/add.svg",
    "icon/user-packages.svg",
    "icon/assets.svg",
    "icon/drag.svg",
    "icon/more.svg",
    "icon/search.svg",
    "icon/chevron-down-small.svg",
    "icon/booth-link.svg",
    "icon/product-menu.svg",
    "icon/archive.svg",
    "icon/file-detail.svg",
    "icon/folder.svg",
    "icon/copy.svg",
    "icon/update.svg",
    "icon/install.svg",
    "icon/cloud-download.svg",
    "icon/check.svg",
    "icon/chevron-down.svg",
    "fonts/GenInterfaceJP-Regular.ttf",
    "fonts/GenInterfaceJP-Medium.ttf",
    "fonts/JetBrainsMono-Regular.ttf",
);

impl Assets {
    pub fn fonts() -> Vec<Cow<'static, [u8]>> {
        FILES
            .iter()
            .filter(|(path, _)| path.starts_with("fonts/"))
            .map(|(_, bytes)| Cow::Borrowed(*bytes))
            .collect()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| (*name).into())
            .collect())
    }
}
