use floem::text::{
    Attrs, AttrsList, FONT_SYSTEM, FamilyOwned, LineHeightValue, TextLayout,
    fontdb::{Database, Query},
};

use crate::config::SparrowConfig;

pub(super) struct TerminalFont {
    family: [FamilyOwned; 1],
    requested_family: String,
    size: usize,
    weight: u16,
    advance: f64,
    scale: f64,
    pub cell_width: f64,
    pub line_height: f64,
    pub baseline: f64,
}

impl TerminalFont {
    pub fn new(config: &SparrowConfig) -> Self {
        let family = config.terminal_font_family();
        let size = config.terminal_font_size();
        let height = config.terminal_line_height();
        let family_choice = resolve_family(FONT_SYSTEM.lock().db(), family);
        let mut font = Self {
            family: [family_choice],
            requested_family: family.to_owned(),
            size,
            weight: font_weight(config),
            advance: 1.0,
            scale: 1.0,
            cell_width: 1.0,
            line_height: height as f64,
            baseline: 0.0,
        };
        let reference =
            TextLayout::new_with_text("W", AttrsList::new(font.attrs()), None);
        font.advance = reference.size().width.max(1.0);
        font.set_scale(1.0);
        font.baseline = reference
            .layout_runs()
            .next()
            .map_or(font.line_height, |run| f64::from(run.line_y));
        font
    }

    pub fn update(&mut self, config: &SparrowConfig) -> bool {
        if self.requested_family == config.terminal_font_family()
            && self.size == config.terminal_font_size()
            && self.line_height == config.terminal_line_height() as f64
            && self.weight == font_weight(config)
        {
            return false;
        }
        let scale = self.scale;
        *self = Self::new(config);
        self.set_scale(scale);
        true
    }

    pub fn set_scale(&mut self, scale: f64) -> bool {
        self.scale = scale;
        // Keep every cell at the same rasterization phase. For example, a
        // 7.8px advance otherwise alternates between seven and eight pixels.
        let cell_width = (self.advance * scale).round().max(1.0) / scale;
        let changed = self.cell_width != cell_width;
        self.cell_width = cell_width;
        changed
    }

    pub fn attrs(&self) -> Attrs<'_> {
        Attrs::new()
            .family(&self.family)
            .font_size(self.size as f32)
            .raw_weight(self.weight)
            .line_height(LineHeightValue::Px(self.line_height as f32))
    }
}

fn font_weight(config: &SparrowConfig) -> u16 {
    match config.terminal.font_weight {
        0 => 500,
        weight => weight.clamp(100, 900),
    }
}

fn resolve_family(db: &Database, families: &str) -> FamilyOwned {
    // Floem's Attrs keeps only the first family it receives.
    FamilyOwned::parse_list(families)
        .chain([
            FamilyOwned::Monospace,
            FamilyOwned::Name("DejaVu Sans Mono".into()),
        ])
        .find(|family| {
            db.query(&Query {
                families: &[family.as_family()],
                ..Query::default()
            })
            .is_some()
        })
        .unwrap_or(FamilyOwned::Monospace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_unavailable_families_and_uses_the_first_installed_one() {
        let mut db = Database::new();
        db.load_font_data(
            include_bytes!(
                "../../../extra/fonts/JetBrainsMono/JetBrainsMono-Regular.ttf"
            )
            .to_vec(),
        );
        db.load_font_data(
            include_bytes!("../../../extra/fonts/DejaVu/DejaVuSansMono.ttf")
                .to_vec(),
        );

        assert_eq!(
            resolve_family(
                &db,
                "Missing Family, 'JetBrains Mono', DejaVu Sans Mono"
            ),
            FamilyOwned::Name("JetBrains Mono".into())
        );
        assert_eq!(
            resolve_family(&db, "Missing Family, DejaVu Sans Mono, JetBrains Mono"),
            FamilyOwned::Name("DejaVu Sans Mono".into())
        );
    }

    #[test]
    fn font_size_and_spacing_updates_recalculate_cell_metrics() {
        let font_name = "DejaVu Sans Mono";
        {
            let mut system = FONT_SYSTEM.lock();
            system.db_mut().load_font_data(
                include_bytes!("../../../extra/fonts/DejaVu/DejaVuSansMono.ttf")
                    .to_vec(),
            );
        }
        let mut config = SparrowConfig::default();
        config.terminal.font_family = font_name.into();
        config.terminal.font_size = 13;
        config.terminal.line_height = 1.4;

        let mut font = TerminalFont::new(&config);
        let original_width = font.cell_width;
        assert_eq!(font.line_height, 18.0);
        assert!(!font.update(&config));

        config.terminal.font_size = 20;
        assert!(font.update(&config));
        assert!(font.cell_width > original_width);
        assert_eq!(font.line_height, 28.0);

        config.terminal.line_height = 34.0;
        assert!(font.update(&config));
        assert_eq!(font.line_height, 34.0);

        config.terminal.line_height = 0.0;
        assert!(font.update(&config));
        assert!(font.line_height >= 20.0);
    }

    #[test]
    fn prompt_uses_real_mono_weights_with_pixel_aligned_cells() {
        use floem::text::{Style, Weight};

        {
            let mut system = FONT_SYSTEM.lock();
            for data in [
                include_bytes!("../../../extra/fonts/JetBrainsMono/JetBrainsMono-Medium.ttf").as_slice(),
                include_bytes!("../../../extra/fonts/JetBrainsMono/JetBrainsMono-MediumItalic.ttf").as_slice(),
                include_bytes!("../../../extra/fonts/JetBrainsMono/JetBrainsMono-Bold.ttf").as_slice(),
                include_bytes!("../../../extra/fonts/JetBrainsMono/JetBrainsMono-BoldItalic.ttf").as_slice(),
            ] {
                system.db_mut().load_font_data(data.to_vec());
            }
        }
        let mut config = SparrowConfig::default();
        config.terminal.font_family = "JetBrains Mono".into();
        config.terminal.font_size = 13;
        config.terminal.font_weight = 500;
        config.terminal.line_height = 1.4;
        let mut font = TerminalFont::new(&config);

        assert_eq!(font.cell_width, 8.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            font.set_scale(scale);
            let advance = font.cell_width * scale;
            assert!((advance - advance.round()).abs() < 1e-9);
        }

        for style in [Style::Normal, Style::Italic] {
            for (weight, attrs) in [
                (Weight(500), font.attrs().style(style)),
                (Weight::BOLD, font.attrs().style(style).weight(Weight::BOLD)),
            ] {
                let layout =
                    TextLayout::new_with_text("git:", AttrsList::new(attrs), None);
                let run = layout.layout_runs().next().unwrap();
                let system = FONT_SYSTEM.lock();
                assert!(!run.glyphs.is_empty());
                for glyph in run.glyphs {
                    let face = system.db().face(glyph.font_id).unwrap();
                    assert_eq!(face.families[0].0, "JetBrains Mono");
                    assert_eq!(face.weight, weight);
                    assert_eq!(face.style, style);
                }
            }
        }
    }
}
