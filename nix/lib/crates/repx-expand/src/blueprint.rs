use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
pub struct Blueprint {
    pub runs: Vec<RunTemplate>,
    pub host_tools: HostTools,
    pub git_hash: String,
    pub repx_version: String,
    pub lab_version: String,
    pub container_mode: ContainerMode,
    pub groups: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub unified_image_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerMode {
    None,
    Unified,
    PerRun,
}

#[derive(Debug, Deserialize)]
pub struct RunTemplate {
    pub name: String,
    pub hash_mode: HashMode,
    pub inter_run_dep_types: BTreeMap<String, String>,
    pub parameter_axes: BTreeMap<String, Vec<serde_json::Value>>,
    pub zip_groups: Vec<ZipGroup>,
    pub pipelines: Vec<PipelineTemplate>,
    pub image_path: Option<String>,
    pub image_contents: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HashMode {
    Pure,
    ParamsOnly,
}

#[derive(Debug, Deserialize)]
pub struct ZipGroup {
    pub members: Vec<String>,
    pub values: Vec<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct PipelineTemplate {
    pub source: String,
    pub stages: Vec<StageTemplate>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StageTemplate {
    pub pname: String,
    pub version: String,
    pub stage_type: StageType,
    pub input_mappings: Vec<InputMapping>,
    pub outputs: BTreeMap<String, serde_json::Value>,
    pub resources: Option<BTreeMap<String, serde_json::Value>>,
    pub executables: BTreeMap<String, ExecutableTemplate>,

    #[serde(default)]
    pub script_drv: Option<String>,

    #[serde(default)]
    pub scatter_drv: Option<String>,
    #[serde(default)]
    pub gather_drv: Option<String>,
    #[serde(default)]
    pub step_drvs: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub step_deps: Option<BTreeMap<String, Vec<String>>>,
}

impl StageTemplate {
    pub fn hash_identities(&self, hash_mode: HashMode) -> Vec<String> {
        match (self.stage_type, hash_mode) {
            (StageType::Simple, HashMode::Pure) => {
                vec![self.script_drv.clone().unwrap_or_default()]
            }
            (StageType::Simple, HashMode::ParamsOnly) => {
                vec![format!("{}-{}", self.pname, self.version)]
            }
            (StageType::ScatterGather, HashMode::Pure) => {
                let mut paths = vec![self.scatter_drv.clone().unwrap_or_default()];
                if let Some(ref step_drvs) = self.step_drvs {
                    let mut step_names: Vec<&String> = step_drvs.keys().collect();
                    step_names.sort();
                    for name in step_names {
                        paths.push(step_drvs[name].clone());
                    }
                }
                paths.push(self.gather_drv.clone().unwrap_or_default());
                paths
            }
            (StageType::ScatterGather, HashMode::ParamsOnly) => {
                let mut ids = vec![format!("{}-scatter-{}", self.pname, self.version)];
                if let Some(ref step_drvs) = self.step_drvs {
                    let mut step_names: Vec<&String> = step_drvs.keys().collect();
                    step_names.sort();
                    for name in step_names {
                        ids.push(format!("{}-step-{name}-{}", self.pname, self.version));
                    }
                }
                ids.push(format!("{}-gather-{}", self.pname, self.version));
                ids
            }
        }
    }

    pub fn all_script_drvs(&self) -> Vec<String> {
        match self.stage_type {
            StageType::Simple => self.script_drv.iter().cloned().collect(),
            StageType::ScatterGather => {
                let mut drvs: Vec<String> = vec![];
                if let Some(ref d) = self.scatter_drv {
                    drvs.push(d.clone());
                }
                if let Some(ref step_drvs) = self.step_drvs {
                    drvs.extend(step_drvs.values().cloned());
                }
                if let Some(ref d) = self.gather_drv {
                    drvs.push(d.clone());
                }
                drvs
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StageType {
    Simple,
    ScatterGather,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutableTemplate {
    pub inputs: Vec<InputMapping>,
    pub outputs: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub resource_hints: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub deps: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InputMapping {
    #[serde(rename = "type", default)]
    pub mapping_type: Option<String>,
    #[serde(default)]
    pub job_id_template: Option<String>,
    #[serde(default)]
    pub source_output: Option<String>,
    pub target_input: String,
    #[serde(default)]
    pub source_run: Option<String>,
    #[serde(default)]
    pub dependency_type: Option<String>,
    #[serde(default)]
    pub source_value: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HostTools {
    pub hash: String,
    pub binaries: Vec<HostToolSpec>,
}

#[derive(Debug, Deserialize)]
pub struct HostToolSpec {
    pub pkg_path: String,
    pub pkg_hash: String,
    pub bins: Option<Vec<BinSpec>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum BinSpec {
    Renamed { src: String, dst: String },
    Simple(String),
}
