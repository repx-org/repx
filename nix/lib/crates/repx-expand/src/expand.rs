use crate::blueprint::{
    HashMode, InputMapping, PipelineTemplate, RunTemplate, StageTemplate, StageType,
};
use crate::cartesian::{build_axes, CartesianIter, ParamCombo};
use crate::nix32::{self, JobId, JobIdHasher};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct ExpandedJob {
    #[serde(skip)]
    pub job_id_bytes: JobId,
    pub job_id: String,
    pub job_name: String,
    pub job_dir_name: String,
    pub pname: String,
    pub stage_type: StageType,
    pub parameters_json: String,
    pub dependency_manifest_json: String,
    pub resolved_parameters: BTreeMap<String, serde_json::Value>,
    pub input_mappings: Vec<InputMapping>,
    pub executables: BTreeMap<String, ExpandedExecutable>,
    pub resources: Option<BTreeMap<String, serde_json::Value>>,
    pub script_sources: Vec<ScriptSource>,
}

#[derive(Debug, Serialize)]
pub struct ExpandedExecutable {
    pub path: String,
    pub inputs: Vec<InputMapping>,
    pub outputs: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_hints: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ScriptSource {
    pub drv_path: String,
    pub bin_name: String,
    pub dest_name: String,
}

#[derive(Debug)]
pub struct ExpandedRun {
    pub name: String,
    pub jobs: Vec<ExpandedJob>,
    pub inter_run_dep_types: BTreeMap<String, String>,
    pub image_contents: Vec<String>,
    pub image_path: Option<String>,
}

pub struct ExpandedLab {
    pub runs: Vec<ExpandedRun>,
    pub blueprint: crate::blueprint::Blueprint,
}

struct ThreadBuffers {
    params_buf: Vec<u8>,
    deps_buf: Vec<u8>,
    mappings_buf: Vec<u8>,
}

impl ThreadBuffers {
    fn new() -> Self {
        Self {
            params_buf: Vec::with_capacity(4096),
            deps_buf: Vec::with_capacity(1024),
            mappings_buf: Vec::with_capacity(2048),
        }
    }

    fn json_to_buf<'a, T: serde::Serialize>(buf: &'a mut Vec<u8>, value: &T) -> &'a [u8] {
        buf.clear();
        serde_json::to_writer(&mut *buf, value)
            .expect("JSON serialization cannot fail for these types");
        buf.as_slice()
    }
}

fn expand_stage(
    stage: &StageTemplate,
    hash_mode: HashMode,
    resolved_parameters: &ParamCombo,
    upstream_job_dir_names: &[String],
    dependency_drvs: &[String],
    bufs: &mut ThreadBuffers,
) -> ExpandedJob {
    let dependency_ids: Vec<&str> = if !upstream_job_dir_names.is_empty() {
        upstream_job_dir_names.iter().map(|s| s.as_str()).collect()
    } else {
        dependency_drvs.iter().map(|s| s.as_str()).collect()
    };

    let dep_manifest_bytes = ThreadBuffers::json_to_buf(&mut bufs.deps_buf, &dependency_ids);
    let dependency_manifest_json = unsafe { std::str::from_utf8_unchecked(dep_manifest_bytes) };

    let dep_hash = {
        let joined = dependency_ids.join(":");
        nix32::sha256_hex(joined.as_bytes())
    };

    let params_bytes = ThreadBuffers::json_to_buf(&mut bufs.params_buf, resolved_parameters);
    let parameters_json = unsafe { std::str::from_utf8_unchecked(params_bytes) };

    let mappings_bytes = ThreadBuffers::json_to_buf(&mut bufs.mappings_buf, &stage.input_mappings);
    let input_mappings_json = unsafe { std::str::from_utf8_unchecked(mappings_bytes) };

    let hash_identities = stage.hash_identities(hash_mode);

    let mut hasher = JobIdHasher::new();
    for identity in &hash_identities {
        hasher.feed_str(identity);
    }
    hasher.feed_str(parameters_json);
    hasher.feed_str(dependency_manifest_json);
    hasher.feed_str(&dep_hash);
    hasher.feed_str(input_mappings_json);

    let job_id_bytes = hasher.finish();
    let job_id = nix32::job_id_str(&job_id_bytes).to_string();
    let job_name = format!("{}-{}", stage.pname, stage.version);
    let job_dir_name = format!("{job_id}-{job_name}");

    let executables = build_executables(stage, &job_dir_name);
    let script_sources = build_script_sources(stage);

    let parameters_json_owned = parameters_json.to_string();
    let dependency_manifest_json_owned = dependency_manifest_json.to_string();

    ExpandedJob {
        job_id_bytes,
        job_id,
        job_name,
        job_dir_name,
        pname: stage.pname.clone(),
        stage_type: stage.stage_type,
        parameters_json: parameters_json_owned,
        dependency_manifest_json: dependency_manifest_json_owned,
        resolved_parameters: resolved_parameters.clone(),
        input_mappings: stage.input_mappings.clone(),
        executables,
        resources: stage.resources.clone(),
        script_sources,
    }
}

fn build_executables(
    stage: &StageTemplate,
    job_dir_name: &str,
) -> BTreeMap<String, ExpandedExecutable> {
    stage
        .executables
        .iter()
        .map(|(exe_name, exe_tmpl)| {
            let path = match stage.stage_type {
                StageType::Simple => format!("jobs/{job_dir_name}/bin/{}", stage.pname),
                StageType::ScatterGather => {
                    format!("jobs/{job_dir_name}/bin/{}-{exe_name}", stage.pname)
                }
            };
            (
                exe_name.clone(),
                ExpandedExecutable {
                    path,
                    inputs: exe_tmpl.inputs.clone(),
                    outputs: exe_tmpl.outputs.clone(),
                    resource_hints: exe_tmpl.resource_hints.clone(),
                    deps: exe_tmpl.deps.clone(),
                },
            )
        })
        .collect()
}

fn build_script_sources(stage: &StageTemplate) -> Vec<ScriptSource> {
    match stage.stage_type {
        StageType::Simple => {
            vec![ScriptSource {
                drv_path: stage.script_drv.clone().unwrap_or_default(),
                bin_name: stage.pname.clone(),
                dest_name: stage.pname.clone(),
            }]
        }
        StageType::ScatterGather => {
            let mut sources = vec![ScriptSource {
                drv_path: stage.scatter_drv.clone().unwrap_or_default(),
                bin_name: format!("{}-scatter", stage.pname),
                dest_name: format!("{}-scatter", stage.pname),
            }];
            if let Some(ref step_drvs) = stage.step_drvs {
                let mut step_names: Vec<&String> = step_drvs.keys().collect();
                step_names.sort();
                for step_name in step_names {
                    sources.push(ScriptSource {
                        drv_path: step_drvs[step_name].clone(),
                        bin_name: format!("{}-step-{step_name}", stage.pname),
                        dest_name: format!("{}-step-{step_name}", stage.pname),
                    });
                }
            }
            sources.push(ScriptSource {
                drv_path: stage.gather_drv.clone().unwrap_or_default(),
                bin_name: format!("{}-gather", stage.pname),
                dest_name: format!("{}-gather", stage.pname),
            });
            sources
        }
    }
}

fn expand_pipeline_for_combo(
    pipeline: &PipelineTemplate,
    hash_mode: HashMode,
    combo: &ParamCombo,
    _inter_run_dep_job_dir_names: &BTreeMap<String, Vec<String>>,
    bufs: &mut ThreadBuffers,
) -> Vec<ExpandedJob> {
    let mut jobs: Vec<ExpandedJob> = Vec::with_capacity(pipeline.stages.len());
    let mut stage_job_dirs: BTreeMap<String, String> = BTreeMap::new();

    for stage in &pipeline.stages {
        let mut upstream_dirs: Vec<String> = Vec::new();
        let mut dependency_drvs: Vec<String> = Vec::new();

        for mapping in &stage.input_mappings {
            match mapping.mapping_type.as_deref().unwrap_or("") {
                "intra-pipeline" => {
                    if let Some(ref upstream_pname) = mapping.job_id_template {
                        if let Some(dir) = stage_job_dirs.get(upstream_pname) {
                            upstream_dirs.push(dir.clone());
                        }
                    }
                }
                "inter-run" => {
                    if let Some(ref source_run) = mapping.source_run {
                        if let Some(dirs) = _inter_run_dep_job_dir_names.get(source_run) {
                            upstream_dirs.extend(dirs.iter().cloned());
                        }
                    }
                }
                _ => {}
            }
        }

        for upstream_dir in &upstream_dirs {
            for prev_job in &jobs {
                if prev_job.job_dir_name == *upstream_dir {
                    for src in &prev_job.script_sources {
                        dependency_drvs.push(src.drv_path.clone());
                    }
                }
            }
        }

        let job = expand_stage(
            stage,
            hash_mode,
            combo,
            &upstream_dirs,
            &dependency_drvs,
            bufs,
        );

        stage_job_dirs.insert(stage.pname.clone(), job.job_dir_name.clone());
        jobs.push(job);
    }

    for job in &mut jobs {
        for exe in job.executables.values_mut() {
            for mapping in &mut exe.inputs {
                if mapping.job_id.is_none() {
                    if let Some(ref template) = mapping.job_id_template {
                        if let Some(dir_name) = stage_job_dirs.get(template) {
                            mapping.job_id = Some(dir_name.clone());
                        }
                    }
                }
            }
        }
        for mapping in &mut job.input_mappings {
            if mapping.job_id.is_none() {
                if let Some(ref template) = mapping.job_id_template {
                    if let Some(dir_name) = stage_job_dirs.get(template) {
                        mapping.job_id = Some(dir_name.clone());
                    }
                }
            }
        }
    }

    jobs
}

const CHUNK_SIZE: u128 = 4096;

pub fn expand_run(
    run: &RunTemplate,
    inter_run_dep_job_dir_names: &BTreeMap<String, Vec<String>>,
) -> ExpandedRun {
    let (axes, total) = build_axes(run);

    if total == 0 {
        return ExpandedRun {
            name: run.name.clone(),
            jobs: vec![],
            inter_run_dep_types: run.inter_run_dep_types.clone(),
            image_contents: run.image_contents.clone(),
            image_path: run.image_path.clone(),
        };
    }

    let iter_template = CartesianIter::new(axes, total);
    let num_chunks = total.div_ceil(CHUNK_SIZE) as usize;

    let all_jobs: Vec<ExpandedJob> = (0..num_chunks)
        .into_par_iter()
        .flat_map(|chunk_idx| {
            let start = chunk_idx as u128 * CHUNK_SIZE;
            let end = ((chunk_idx as u128 + 1) * CHUNK_SIZE).min(total);

            let mut bufs = ThreadBuffers::new();
            let mut chunk_jobs: Vec<ExpandedJob> =
                Vec::with_capacity((end - start) as usize * run.pipelines.len() * 8);

            for combo_idx in start..end {
                let combo = iter_template.combo_at(combo_idx);
                for pipeline in &run.pipelines {
                    let jobs = expand_pipeline_for_combo(
                        pipeline,
                        run.hash_mode,
                        &combo,
                        inter_run_dep_job_dir_names,
                        &mut bufs,
                    );
                    chunk_jobs.extend(jobs);
                }
            }

            chunk_jobs
        })
        .collect();

    ExpandedRun {
        name: run.name.clone(),
        jobs: all_jobs,
        inter_run_dep_types: run.inter_run_dep_types.clone(),
        image_contents: run.image_contents.clone(),
        image_path: run.image_path.clone(),
    }
}

pub fn expand_blueprint(blueprint: crate::blueprint::Blueprint) -> ExpandedLab {
    let mut runs: Vec<ExpandedRun> = Vec::with_capacity(blueprint.runs.len());
    let mut inter_run_job_dirs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for run_template in &blueprint.runs {
        let dep_dirs: BTreeMap<String, Vec<String>> = run_template
            .inter_run_dep_types
            .keys()
            .filter_map(|dep_name| {
                inter_run_job_dirs
                    .get(dep_name)
                    .map(|dirs| (dep_name.clone(), dirs.clone()))
            })
            .collect();

        let expanded = expand_run(run_template, &dep_dirs);

        let job_dirs: Vec<String> = expanded
            .jobs
            .iter()
            .map(|j| j.job_dir_name.clone())
            .collect();
        inter_run_job_dirs.insert(expanded.name.clone(), job_dirs);

        runs.push(expanded);
    }

    ExpandedLab { runs, blueprint }
}
