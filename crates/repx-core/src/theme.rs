use crate::config::Config;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use xdg::BaseDirectories;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementStyle {
    pub color: String,
    #[serde(default)]
    pub styles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusStyles {
    pub succeeded: ElementStyle,
    pub failed: ElementStyle,
    pub submit_failed: ElementStyle,
    pub pending: ElementStyle,
    pub queued: ElementStyle,
    pub blocked: ElementStyle,
    pub running: ElementStyle,
    pub submitting: ElementStyle,
    pub unknown: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentifierStyles {
    pub run_id: ElementStyle,
    pub job_hash: ElementStyle,
    pub job_name: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureStyles {
    pub tree_branch: ElementStyle,
    pub labels: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryStyles {
    pub success: ElementStyle,
    pub info: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelStyles {
    pub overview: ElementStyle,
    pub targets: ElementStyle,
    pub context: ElementStyle,
    pub logs: ElementStyle,
    pub runs_jobs: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStyles {
    pub background: ElementStyle,
    pub rate_low: ElementStyle,
    pub rate_high: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStyles {
    pub row_highlight_fg: ElementStyle,
    pub row_highlight_bg: ElementStyle,
    pub cell_highlight_fg: ElementStyle,
    pub cell_highlight_bg: ElementStyle,
    pub selector: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetStateStyles {
    pub active: ElementStyle,
    pub inactive: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopupStyles {
    pub border: ElementStyle,
    pub key_fg: ElementStyle,
    pub key_bg: ElementStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Elements {
    pub job_status: JobStatusStyles,
    pub identifiers: IdentifierStyles,
    pub structure: StructureStyles,
    pub summary: SummaryStyles,
    pub panels: PanelStyles,
    pub graphs: GraphStyles,
    pub tables: TableStyles,
    pub target_states: TargetStateStyles,
    pub popups: PopupStyles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbols {
    pub tree_vertical: String,
    pub tree_junction: String,
    pub tree_end: String,
}

pub type Palette = BTreeMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
    pub elements: Elements,
    pub symbols: Symbols,
}

fn dracula_theme() -> Theme {
    Theme {
        name: "Dracula".to_string(),
        palette: BTreeMap::from([
            ("background".to_string(), "#282a36".to_string()),
            ("black".to_string(), "#000000".to_string()),
            ("current_line".to_string(), "#44475a".to_string()),
            ("selection".to_string(), "#44475a".to_string()),
            ("foreground".to_string(), "#f8f8f2".to_string()),
            ("comment".to_string(), "#6272a4".to_string()),
            ("cyan".to_string(), "#8be9fd".to_string()),
            ("green".to_string(), "#50fa7b".to_string()),
            ("orange".to_string(), "#ffb86c".to_string()),
            ("pink".to_string(), "#ff79c6".to_string()),
            ("purple".to_string(), "#bd93f9".to_string()),
            ("red".to_string(), "#ff5555".to_string()),
            ("yellow".to_string(), "#f1fa8c".to_string()),
        ]),
        elements: Elements {
            job_status: JobStatusStyles {
                succeeded: ElementStyle {
                    color: "green".to_string(),
                    styles: vec![],
                },
                failed: ElementStyle {
                    color: "red".to_string(),
                    styles: vec![],
                },
                submit_failed: ElementStyle {
                    color: "red".to_string(),
                    styles: vec!["bold".to_string()],
                },
                pending: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
                queued: ElementStyle {
                    color: "purple".to_string(),
                    styles: vec![],
                },
                blocked: ElementStyle {
                    color: "orange".to_string(),
                    styles: vec![],
                },
                running: ElementStyle {
                    color: "cyan".to_string(),
                    styles: vec![],
                },
                submitting: ElementStyle {
                    color: "pink".to_string(),
                    styles: vec!["bold".to_string()],
                },
                unknown: ElementStyle {
                    color: "comment".to_string(),
                    styles: vec![],
                },
            },
            identifiers: IdentifierStyles {
                run_id: ElementStyle {
                    color: "foreground".to_string(),
                    styles: vec![],
                },
                job_hash: ElementStyle {
                    color: "cyan".to_string(),
                    styles: vec![],
                },
                job_name: ElementStyle {
                    color: "foreground".to_string(),
                    styles: vec![],
                },
            },
            structure: StructureStyles {
                tree_branch: ElementStyle {
                    color: "comment".to_string(),
                    styles: vec!["dimmed".to_string()],
                },
                labels: ElementStyle {
                    color: "foreground".to_string(),
                    styles: vec!["bold".to_string()],
                },
            },
            summary: SummaryStyles {
                success: ElementStyle {
                    color: "green".to_string(),
                    styles: vec![],
                },
                info: ElementStyle {
                    color: "foreground".to_string(),
                    styles: vec![],
                },
            },
            panels: PanelStyles {
                overview: ElementStyle {
                    color: "pink".to_string(),
                    styles: vec![],
                },
                targets: ElementStyle {
                    color: "comment".to_string(),
                    styles: vec![],
                },
                context: ElementStyle {
                    color: "green".to_string(),
                    styles: vec![],
                },
                logs: ElementStyle {
                    color: "red".to_string(),
                    styles: vec![],
                },
                runs_jobs: ElementStyle {
                    color: "cyan".to_string(),
                    styles: vec![],
                },
            },
            graphs: GraphStyles {
                background: ElementStyle {
                    color: "background".to_string(),
                    styles: vec![],
                },
                rate_low: ElementStyle {
                    color: "orange".to_string(),
                    styles: vec![],
                },
                rate_high: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
            },
            tables: TableStyles {
                row_highlight_fg: ElementStyle {
                    color: "black".to_string(),
                    styles: vec!["bold".to_string()],
                },
                row_highlight_bg: ElementStyle {
                    color: "cyan".to_string(),
                    styles: vec![],
                },
                cell_highlight_fg: ElementStyle {
                    color: "black".to_string(),
                    styles: vec![],
                },
                cell_highlight_bg: ElementStyle {
                    color: "comment".to_string(),
                    styles: vec![],
                },
                selector: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
            },
            target_states: TargetStateStyles {
                active: ElementStyle {
                    color: "green".to_string(),
                    styles: vec!["bold".to_string()],
                },
                inactive: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
            },
            popups: PopupStyles {
                border: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
                key_fg: ElementStyle {
                    color: "black".to_string(),
                    styles: vec!["bold".to_string()],
                },
                key_bg: ElementStyle {
                    color: "yellow".to_string(),
                    styles: vec![],
                },
            },
        },
        symbols: Symbols {
            tree_vertical: "│".to_string(),
            tree_junction: "├───".to_string(),
            tree_end: "└───".to_string(),
        },
    }
}

pub fn default_theme() -> Theme {
    dracula_theme()
}

fn merge_values(a: &mut toml::Value, b: &toml::Value) {
    if let toml::Value::Table(a) = a {
        if let toml::Value::Table(b) = b {
            for (k, v) in b {
                merge_values(a.entry(k.clone()).or_insert(v.clone()), v);
            }
            return;
        }
    }
    *a = b.clone();
}

pub fn load_theme(config: &Config) -> Result<Theme, AppError> {
    let mut base_theme = match config.theme.as_deref() {
        Some("dracula") => dracula_theme(),
        _ => default_theme(),
    };

    let xdg_dirs = BaseDirectories::with_prefix("repx");
    if let Some(theme_path) = xdg_dirs.find_config_file("theme.toml") {
        let user_theme_str = fs::read_to_string(theme_path)?;
        let user_theme_value: toml::Value = toml::from_str(&user_theme_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut base_theme_value =
            toml::Value::try_from(&base_theme).map_err(std::io::Error::other)?;

        merge_values(&mut base_theme_value, &user_theme_value);

        base_theme = base_theme_value
            .try_into::<Theme>()
            .map_err(std::io::Error::other)?;
    }

    Ok(base_theme)
}
