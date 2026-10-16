//! Machine details for the GUI header: model, motherboard, BIOS and fan board.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemInfo {
    /// e.g. "Thelio Mira (thelio-mira-r5)"
    pub model: Option<String>,
    /// e.g. "X870 MB"
    pub motherboard: Option<String>,
    /// e.g. "F13b Z5 (06/17/2026)"
    pub bios: Option<String>,
    pub bios_vendor: Option<String>,
    /// e.g. "Thelio Io 2"; None when no Thelio Io board is connected
    pub fan_board: Option<String>,
}

impl SystemInfo {
    pub fn detect() -> Self {
        let mut info =
            from_dmi(|field| fs::read_to_string(Path::new("/sys/class/dmi/id").join(field)).ok());
        info.fan_board = fan_board();
        info
    }
}

/// Model, motherboard and BIOS from DMI fields read by `read`.
fn from_dmi(read: impl Fn(&str) -> Option<String>) -> SystemInfo {
    let field = |name: &str| {
        read(name)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && v != "To Be Filled By O.E.M.")
    };

    let product = field("product_name");
    let model = match (product.clone(), field("product_version")) {
        (Some(name), Some(version)) if version != name => Some(format!("{} ({})", name, version)),
        (name, version) => name.or(version),
    };

    // System76 names the board after the product, so the chipset is in
    // product_family; other boards have their own name
    let motherboard = match field("board_name") {
        Some(board) if Some(&board) != product.as_ref() => Some(
            field("board_vendor").map_or(board.clone(), |vendor| format!("{} {}", vendor, board)),
        ),
        _ => field("product_family"),
    };

    let bios = match (field("bios_version"), field("bios_date")) {
        (Some(version), Some(date)) => Some(format!("{} ({})", version, date)),
        (version, date) => version.or(date),
    };

    SystemInfo {
        model,
        motherboard,
        bios,
        bios_vendor: field("bios_vendor"),
        fan_board: None,
    }
}

/// The Thelio Io board's product name, from its USB device when available.
pub fn fan_board() -> Option<String> {
    let entries = fs::read_dir("/sys/class/hwmon").ok()?;
    entries.flatten().find_map(|entry| {
        let name = fs::read_to_string(entry.path().join("name")).ok()?;
        let fallback = match name.trim() {
            "system76_thelio_io" => "Thelio Io 2",
            "system76_io" => "Thelio Io",
            _ => return None,
        };
        // hwmon device -> HID interface -> USB device
        let usb_product = fs::canonicalize(entry.path().join("device"))
            .ok()
            .and_then(|hid| fs::read_to_string(hid.join("../../product")).ok())
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        Some(usb_product.unwrap_or_else(|| fallback.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn dmi(fields: &[(&str, &str)]) -> SystemInfo {
        let fields: HashMap<String, String> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), format!("{}\n", v)))
            .collect();
        from_dmi(|name| fields.get(name).cloned())
    }

    #[test]
    fn thelio_shows_model_revision_chipset_and_bios() {
        let info = dmi(&[
            ("product_name", "Thelio Mira"),
            ("product_version", "thelio-mira-r5"),
            ("product_family", "X870 MB"),
            ("board_vendor", "System76"),
            ("board_name", "Thelio Mira"),
            ("bios_vendor", "American Megatrends International, LLC."),
            ("bios_version", "F13b Z5"),
            ("bios_date", "06/17/2026"),
        ]);
        assert_eq!(info.model.as_deref(), Some("Thelio Mira (thelio-mira-r5)"));
        assert_eq!(info.motherboard.as_deref(), Some("X870 MB"));
        assert_eq!(info.bios.as_deref(), Some("F13b Z5 (06/17/2026)"));
    }

    #[test]
    fn other_boards_use_their_own_name() {
        let info = dmi(&[
            ("product_name", "Some PC"),
            ("board_vendor", "Gigabyte Technology Co., Ltd."),
            ("board_name", "X870 AORUS ELITE"),
            ("bios_version", "F5"),
        ]);
        assert_eq!(info.model.as_deref(), Some("Some PC"));
        assert_eq!(
            info.motherboard.as_deref(),
            Some("Gigabyte Technology Co., Ltd. X870 AORUS ELITE")
        );
        assert_eq!(info.bios.as_deref(), Some("F5"));
    }

    #[test]
    fn placeholder_and_missing_fields_are_skipped() {
        let info = dmi(&[
            ("product_name", "To Be Filled By O.E.M."),
            ("product_version", ""),
        ]);
        assert_eq!(info, SystemInfo::default());
    }
}
