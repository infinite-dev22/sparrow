//! Published panel resolutions and the corresponding preview viewports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Display {
    pub pixels: (u32, u32),
    pub logical: (u32, u32),
    pub scale: f64,
    pub derived: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Device {
    pub name: &'static str,
    pub display: Display,
    pub cover: Option<Display>,
    pub ios: bool,
}

const fn android(width: u32, height: u32, ppi: f64) -> Display {
    // A reproducible dp preview from the published physical density and Android's
    // 160 dpi baseline. OEM user-selectable display scaling can change actual dp.
    let scale = ppi / 160.0;
    Display {
        pixels: (width, height),
        logical: (
            (width as f64 / scale + 0.5) as u32,
            (height as f64 / scale + 0.5) as u32,
        ),
        scale,
        derived: true,
    }
}
const fn retina(pixels: (u32, u32), logical: (u32, u32), scale: f64) -> Display {
    Display {
        pixels,
        logical,
        scale,
        derived: false,
    }
}
const fn phone(name: &'static str, display: Display, ios: bool) -> Device {
    Device {
        name,
        display,
        cover: None,
        ios,
    }
}
const fn fold(name: &'static str, display: Display, cover: Display) -> Device {
    Device {
        name,
        display,
        cover: Some(cover),
        ios: false,
    }
}

pub(super) const DEVICES: &[Device] = &[
    // Google Pixel Help hardware specs (7158570). Pixel 10 Pro XL uses the Android
    // SDK AVD profile's 480 dpi setting. The Fold uses Google's two panel specs.
    phone("Pixel 10", android(1080, 2424, 422.0), false),
    phone("Pixel 10a", android(1080, 2424, 422.2), false),
    phone("Pixel 10 Pro", android(1280, 2856, 495.0), false),
    phone(
        "Pixel 10 Pro XL",
        retina((1344, 2992), (448, 997), 3.0),
        false,
    ),
    fold(
        "Pixel 10 Pro Fold",
        android(2076, 2152, 373.0),
        android(1080, 2364, 408.0),
    ),
    phone("Pixel 8", android(1080, 2400, 428.0), false),
    phone("Pixel 8a", android(1080, 2400, 430.0), false),
    phone("Pixel 8 Pro", android(1344, 2992, 489.0), false),
    // Samsung Electronics 2025 Business Report, p. 335, and Samsung S23 FE specs.
    phone("Galaxy S23", android(1080, 2340, 425.0), false),
    phone("Galaxy S23+", android(1080, 2340, 393.0), false),
    phone("Galaxy S23 Ultra", android(1440, 3088, 500.0), false),
    phone("Galaxy S23 FE", android(1080, 2340, 403.0), false),
    // Apple iOS Device Compatibility Reference and individual Support tech specs.
    phone("iPhone 8", retina((750, 1334), (375, 667), 2.0), true),
    phone("iPhone 8 Plus", retina((1080, 1920), (414, 736), 3.0), true),
    phone("iPhone X", retina((1125, 2436), (375, 812), 3.0), true),
    phone("iPhone XS", retina((1125, 2436), (375, 812), 3.0), true),
    phone("iPhone XS Max", retina((1242, 2688), (414, 896), 3.0), true),
    phone("iPhone XR", retina((828, 1792), (414, 896), 2.0), true),
    phone(
        "iPhone 12 mini",
        retina((1080, 2340), (375, 812), 3.0),
        true,
    ),
    phone("iPhone 12", retina((1170, 2532), (390, 844), 3.0), true),
    phone("iPhone 12 Pro", retina((1170, 2532), (390, 844), 3.0), true),
    phone(
        "iPhone 12 Pro Max",
        retina((1284, 2778), (428, 926), 3.0),
        true,
    ),
    phone("iPhone 18 Pro", retina((1206, 2622), (402, 874), 3.0), true),
    phone(
        "iPhone 18 Pro Max",
        retina((1320, 2868), (440, 956), 3.0),
        true,
    ),
    // Apple Support 148588. Map the two panels to a 3× Retina preview.
    Device {
        name: "iPhone Duo",
        display: retina((2670, 1878), (890, 626), 3.0),
        cover: Some(retina((1398, 2034), (466, 678), 3.0)),
        ios: true,
    },
    // Samsung Newsroom Galaxy Z Fold6 and Z Flip6 launch specifications.
    fold(
        "Galaxy Z Fold6",
        android(1856, 2160, 374.0),
        android(968, 2376, 410.0),
    ),
    fold(
        "Galaxy Z Flip6",
        android(1080, 2640, 425.0),
        android(720, 748, 306.0),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Viewport {
    pub size: (u32, u32),
    pub scale: f64,
    pub landscape: bool,
    pub dark: bool,
    pub ios: bool,
    pub mobile: bool,
}

impl Viewport {
    pub fn new(display: Display, landscape: bool, dark: bool, ios: bool) -> Self {
        let (w, h) = display.logical;
        Self {
            size: if landscape { (h, w) } else { (w, h) },
            scale: display.scale,
            landscape,
            dark,
            ios,
            mobile: true,
        }
    }

    pub fn custom(width: &str, height: &str, scale: &str) -> Option<Display> {
        let width: u32 = width.trim().parse().ok()?;
        let height: u32 = height.trim().parse().ok()?;
        let scale: f64 = scale.trim().parse().ok()?;
        if !(200..=2000).contains(&width)
            || !(200..=2500).contains(&height)
            || !(0.5..=4.0).contains(&scale)
        {
            return None;
        }
        Some(retina(
            (
                (width as f64 * scale).round() as u32,
                (height as f64 * scale).round() as u32,
            ),
            (width, height),
            scale,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_preserves_device_scale_and_fold_displays_are_distinct() {
        let device = DEVICES
            .iter()
            .find(|d| d.name == "Pixel 10 Pro Fold")
            .unwrap();
        let inner = Viewport::new(device.display, true, true, false);
        let outer = Viewport::new(device.cover.unwrap(), false, false, false);
        assert_eq!(device.display.pixels, (2076, 2152));
        assert_eq!(inner.size.0, device.display.logical.1);
        assert_eq!(inner.scale, device.display.scale);
        assert_ne!(inner.size, outer.size);
    }

    #[test]
    fn flexible_dimensions_are_bounded_and_require_a_finite_scale() {
        assert_eq!(
            Viewport::custom("400", "800", "2.5").unwrap().pixels,
            (1000, 2000)
        );
        assert!(Viewport::custom("0", "800", "2").is_none());
        assert!(Viewport::custom("400", "800", "NaN").is_none());
    }
}
