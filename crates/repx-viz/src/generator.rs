use anyhow::Result;
use repx_core::model::{Job, JobId, Lab, StageType};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::helpers::*;
use crate::VizArgs;

pub(crate) struct VizGenerator<'a> {
    pub lab: &'a Lab,
    scatter_gather_clean_names: HashSet<String>,
}

impl<'a> VizGenerator<'a> {
    pub fn new(lab: &'a Lab) -> Self {
        let scatter_gather_clean_names = lab
            .jobs
            .values()
            .filter(|job| job.stage_type == StageType::ScatterGather)
            .filter_map(|job| job.name.as_ref())
            .map(|name| clean_id(name))
            .collect();

        Self {
            lab,
            scatter_gather_clean_names,
        }
    }

    #[allow(clippy::expect_used)]
    pub fn generate_dot(&mut self, args: &VizArgs) -> Result<String> {
        let mut dot = String::new();
        dot.push_str("digraph \"RepX Topology\" {\n");

        if args.format.as_deref() != Some("svg") {
            dot_writeln!(dot, "    dpi=\"{}\";", DPI);
        }
        dot.push_str("    compound=\"true\";\n");
        dot.push_str("    rankdir=\"LR\";\n");
        dot.push_str("    bgcolor=\"#FFFFFF\";\n");
        dot_writeln!(dot, "    pad=\"{}\";", GRAPH_PAD);
        dot_writeln!(dot, "    nodesep=\"{}\";", NODE_SEP);
        dot_writeln!(dot, "    ranksep=\"{}\";", RANK_SEP);
        dot_writeln!(dot, "    node [fontname=\"{}\"];", FONT_NAME);
        dot.push_str("    edge [color=\"#000000\", penwidth=\"1.2\", arrowsize=\"0.7\"];\n\n");

        let mut job_to_run: HashMap<JobId, String> = HashMap::new();
        for (run_id, run) in &self.lab.runs {
            for jid in &run.jobs {
                job_to_run.insert(jid.clone(), run_id.to_string());
            }
        }

        let mut pipeline_jobs: BTreeMap<String, Vec<&JobId>> = BTreeMap::new();
        let mut pipeline_representative: HashMap<String, &Job> = HashMap::new();

        for (jid, job) in &self.lab.jobs {
            let name = job.name.clone().unwrap_or_else(|| jid.to_string());
            pipeline_jobs.entry(name.clone()).or_default().push(jid);
            pipeline_representative.entry(name).or_insert(job);
        }

        let mut run_pipelines: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (jid, job) in &self.lab.jobs {
            let run_name = job_to_run
                .get(jid)
                .cloned()
                .unwrap_or_else(|| "detached".to_string());
            let pipeline_name = job.name.clone().unwrap_or_else(|| jid.to_string());
            run_pipelines
                .entry(run_name)
                .or_default()
                .insert(pipeline_name);
        }

        if args.show_pipelines {
            self.render_pipeline_layer(&mut dot, args, &pipeline_jobs, &pipeline_representative);
        }

        if args.show_runs {
            let mut run_to_group: HashMap<String, String> = HashMap::new();
            if args.show_groups {
                for (group_name, run_ids) in &self.lab.groups {
                    for rid in run_ids {
                        run_to_group.insert(rid.to_string(), group_name.clone());
                    }
                }
            }

            self.render_run_layer(&mut dot, args, &run_pipelines, &run_to_group);
        }

        if args.show_pipelines && args.show_inter_edges {
            self.render_inter_run_edges(&mut dot, &job_to_run);
        }

        if args.show_pipelines && args.show_runs {
            for (run_name, pipelines) in &run_pipelines {
                let clean_run = clean_id(run_name);
                let run_node = format!("run_{}", clean_run);
                for pipeline_name in pipelines {
                    let clean_pipe = clean_id(pipeline_name);
                    let is_sg = self.scatter_gather_clean_names.contains(&clean_pipe);
                    let pipe_node = if is_sg {
                        format!("pipe_{}_sg_scatter", clean_pipe)
                    } else {
                        format!("pipe_{}", clean_pipe)
                    };
                    dot_writeln!(dot, "    {} -> {} [", run_node, pipe_node);
                    dot.push_str("        style=\"dashed\",\n");
                    dot.push_str("        color=\"#94a3b8\",\n");
                    dot.push_str("        arrowhead=\"open\",\n");
                    dot.push_str("        penwidth=\"0.8\"\n");
                    dot.push_str("    ];\n");
                }
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    fn render_pipeline_layer(
        &self,
        dot: &mut String,
        args: &VizArgs,
        pipeline_jobs: &BTreeMap<String, Vec<&JobId>>,
        pipeline_representative: &HashMap<String, &Job>,
    ) {
        for (pipeline_name, job_ids) in pipeline_jobs {
            let count = job_ids.len();
            let clean_pipe = clean_id(pipeline_name);
            let node_id = format!("pipe_{}", clean_pipe);

            let is_sg = pipeline_representative
                .get(pipeline_name)
                .map(|j| j.stage_type == StageType::ScatterGather)
                .unwrap_or(false);

            if is_sg {
                #[allow(clippy::expect_used)]
                let rep = pipeline_representative
                    .get(pipeline_name)
                    .expect("representative must exist if pipeline is in map");
                self.render_scatter_gather_subgraph(
                    dot,
                    pipeline_name,
                    &node_id,
                    count,
                    rep,
                    "    ",
                );
            } else {
                let job_label = format!("{}\\n(x{})", escape_dot_label(pipeline_name), count);
                let fill_color = get_fill_color(pipeline_name);

                dot_writeln!(dot, "    {} [", node_id);
                dot_writeln!(dot, "        label=\"{}\",", job_label);
                dot.push_str("        shape=\"box\",\n");
                dot.push_str("        style=\"filled,rounded\",\n");
                dot_writeln!(dot, "        fontsize=\"{}\",", JOB_FONT_SIZE);
                dot_writeln!(dot, "        fillcolor=\"{}\",", fill_color);
                dot.push_str("        penwidth=\"1\"\n");
                dot.push_str("    ];\n");
            }

            if args.show_params {
                let varying = self.get_varying_params(job_ids);
                for (p_key, p_vals) in varying {
                    let clean_key = clean_id(&p_key);
                    let param_node_id = format!("pparam_{}_{}", clean_pipe, clean_key);

                    let clean_vals: Vec<String> = p_vals
                        .iter()
                        .map(|v| smart_truncate(v, PARAM_MAX_WIDTH))
                        .collect();
                    let mut val_str = clean_vals.join(", ");
                    if val_str.chars().count() > PARAM_MAX_WIDTH {
                        let keep = PARAM_MAX_WIDTH.saturating_sub(2);
                        let truncated: String = val_str.chars().take(keep).collect();
                        val_str = format!("{}..", truncated);
                    }
                    let label = format!(
                        "{}:\\n{}",
                        escape_dot_label(&p_key),
                        escape_dot_label(&val_str)
                    );

                    dot_writeln!(dot, "    {} [", param_node_id);
                    dot_writeln!(dot, "        label=\"{}\",", label);
                    dot_writeln!(dot, "        shape=\"{}\",", PARAM_SHAPE);
                    dot.push_str("        style=\"filled\",\n");
                    dot_writeln!(dot, "        fillcolor=\"{}\",", PARAM_FILL);
                    dot_writeln!(dot, "        color=\"{}\",", PARAM_BORDER);
                    dot_writeln!(dot, "        fontcolor=\"{}\",", PARAM_FONT_COLOR);
                    dot_writeln!(dot, "        fontsize=\"{}\",", PARAM_FONT_SIZE);
                    dot.push_str("        margin=\"0.1,0.05\",\n");
                    dot.push_str("        penwidth=\"0.8\"\n");
                    dot.push_str("    ];\n");

                    let target = if is_sg {
                        format!("pipe_{}_sg_scatter", clean_pipe)
                    } else {
                        node_id.clone()
                    };
                    dot_writeln!(dot, "    {} -> {} [", param_node_id, target);
                    dot.push_str("        style=\"dotted\",\n");
                    dot_writeln!(dot, "        color=\"{}\",", PARAM_BORDER);
                    dot.push_str("        arrowhead=\"dot\",\n");
                    dot.push_str("        arrowsize=\"0.5\",\n");
                    dot.push_str("        penwidth=\"1.0\"\n");
                    dot.push_str("    ];\n");
                }
            }
        }

        if args.show_intra_edges {
            let mut drawn: HashSet<(String, String)> = HashSet::new();

            for job in self.lab.jobs.values() {
                let tgt_name = job.name.clone().unwrap_or_default();
                let clean_tgt = clean_id(&tgt_name);

                for mapping in Self::get_job_inputs(job) {
                    if let Some(sid) = &mapping.job_id {
                        if let Some(src_job) = self.lab.jobs.get(sid) {
                            let src_name = src_job.name.clone().unwrap_or_default();
                            let clean_src = clean_id(&src_name);

                            if clean_src == clean_tgt {
                                continue;
                            }

                            let key = (clean_src.clone(), clean_tgt.clone());
                            if drawn.contains(&key) {
                                continue;
                            }
                            drawn.insert(key);

                            let src_is_sg = self.scatter_gather_clean_names.contains(&clean_src);
                            let dst_is_sg = self.scatter_gather_clean_names.contains(&clean_tgt);

                            let actual_src = if src_is_sg {
                                format!("pipe_{}_sg_gather", clean_src)
                            } else {
                                format!("pipe_{}", clean_src)
                            };
                            let actual_dst = if dst_is_sg {
                                format!("pipe_{}_sg_scatter", clean_tgt)
                            } else {
                                format!("pipe_{}", clean_tgt)
                            };

                            dot_write!(
                                dot,
                                "    {} -> {} [penwidth=\"1.2\"",
                                actual_src,
                                actual_dst
                            );
                            if src_is_sg {
                                dot_write!(dot, ", ltail=\"cluster_pipe_{}_sg\"", clean_src);
                            }
                            if dst_is_sg {
                                dot_write!(dot, ", lhead=\"cluster_pipe_{}_sg\"", clean_tgt);
                            }
                            dot.push_str("];\n");
                        }
                    }
                }
            }
        }
    }

    fn render_run_layer(
        &self,
        dot: &mut String,
        args: &VizArgs,
        run_pipelines: &BTreeMap<String, BTreeSet<String>>,
        run_to_group: &HashMap<String, String>,
    ) {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ungrouped: Vec<String> = Vec::new();

        for run_name in run_pipelines.keys() {
            if let Some(group) = run_to_group.get(run_name) {
                groups
                    .entry(group.clone())
                    .or_default()
                    .push(run_name.clone());
            } else {
                ungrouped.push(run_name.clone());
            }
        }
        ungrouped.sort();

        if args.show_groups {
            for (group_name, mut run_names) in groups {
                run_names.sort();
                let clean_group = clean_id(&group_name);
                dot_writeln!(dot, "    subgraph cluster_group_{} {{", clean_group);
                dot_writeln!(dot, "        label=\"@{}\";", escape_dot_label(&group_name));
                dot.push_str("        style=\"solid,rounded\";\n");
                dot_writeln!(dot, "        color=\"{}\";", COLOR_GROUP_BORDER);
                dot_writeln!(dot, "        fontsize=\"{}\";", GROUP_FONT_SIZE);
                dot.push_str("        penwidth=\"2\";\n");
                dot.push_str("        margin=\"20\";\n\n");

                for run_name in &run_names {
                    if let Some(pipelines) = run_pipelines.get(run_name) {
                        self.render_run_summary_node(dot, run_name, pipelines, "        ");
                    }
                }

                dot.push_str("    }\n\n");
            }
        } else {
            for (_group_name, mut run_names) in groups {
                run_names.sort();
                for run_name in &run_names {
                    if let Some(pipelines) = run_pipelines.get(run_name) {
                        self.render_run_summary_node(dot, run_name, pipelines, "    ");
                    }
                }
            }
        }

        for run_name in &ungrouped {
            if let Some(pipelines) = run_pipelines.get(run_name) {
                self.render_run_summary_node(dot, run_name, pipelines, "    ");
            }
        }
    }

    fn render_run_summary_node(
        &self,
        dot: &mut String,
        run_name: &str,
        pipelines: &BTreeSet<String>,
        indent: &str,
    ) {
        let clean_run = clean_id(run_name);
        let node_id = format!("run_{}", clean_run);

        let pipe_list = pipelines
            .iter()
            .map(|p| escape_dot_label(p))
            .collect::<Vec<_>>()
            .join(", ");

        let label = format!(
            "{}|{{pipelines: {}}}",
            escape_dot_label(run_name),
            pipe_list
        );

        dot_writeln!(dot, "{}{} [", indent, node_id);
        dot_writeln!(dot, "{}    label=\"{}\",", indent, label);
        dot.push_str(&format!("{}    shape=\"record\",\n", indent));
        dot.push_str(&format!("{}    style=\"filled,rounded\",\n", indent));
        dot_writeln!(dot, "{}    fillcolor=\"{}\",", indent, RUN_FILL);
        dot_writeln!(dot, "{}    color=\"{}\",", indent, COLOR_CLUSTER_BORDER);
        dot_writeln!(dot, "{}    fontsize=\"{}\",", indent, JOB_FONT_SIZE);
        dot.push_str(&format!("{}    penwidth=\"1.5\"\n", indent));
        dot_writeln!(dot, "{}];", indent);
    }

    fn render_inter_run_edges(&self, dot: &mut String, job_to_run: &HashMap<JobId, String>) {
        let mut drawn: HashSet<(String, String, String)> = HashSet::new();

        for job in self.lab.jobs.values() {
            let tgt_name = job.name.clone().unwrap_or_default();
            let clean_tgt = clean_id(&tgt_name);

            for mapping in Self::get_job_inputs(job) {
                if let Some(srun) = &mapping.source_run {
                    let dtype = mapping
                        .dependency_type
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "hard".to_string());

                    let src_pipelines: BTreeSet<String> = self
                        .lab
                        .runs
                        .get(srun)
                        .map(|run| {
                            run.jobs
                                .iter()
                                .filter_map(|jid| self.lab.jobs.get(jid))
                                .filter_map(|j| j.name.clone())
                                .collect()
                        })
                        .unwrap_or_default();

                    let filtered: BTreeSet<String> =
                        if let Some(filter) = &mapping.source_stage_filter {
                            src_pipelines.into_iter().filter(|n| n == filter).collect()
                        } else {
                            src_pipelines
                        };

                    for src_pipeline in filtered {
                        let clean_src = clean_id(&src_pipeline);
                        let key = (clean_src.clone(), clean_tgt.clone(), dtype.clone());
                        if drawn.contains(&key) {
                            continue;
                        }
                        drawn.insert(key);

                        let src_is_sg = self.scatter_gather_clean_names.contains(&clean_src);
                        let dst_is_sg = self.scatter_gather_clean_names.contains(&clean_tgt);

                        let actual_src = if src_is_sg {
                            format!("pipe_{}_sg_gather", clean_src)
                        } else {
                            format!("pipe_{}", clean_src)
                        };
                        let actual_dst = if dst_is_sg {
                            format!("pipe_{}_sg_scatter", clean_tgt)
                        } else {
                            format!("pipe_{}", clean_tgt)
                        };

                        let style = if dtype == "soft" { "dashed" } else { "solid" };
                        dot_writeln!(dot, "    {} -> {} [", actual_src, actual_dst);
                        if src_is_sg {
                            dot_writeln!(dot, "        ltail=\"cluster_pipe_{}_sg\",", clean_src);
                        }
                        if dst_is_sg {
                            dot_writeln!(dot, "        lhead=\"cluster_pipe_{}_sg\",", clean_tgt);
                        }
                        dot_writeln!(dot, "        style=\"{}\",", style);
                        dot.push_str("        color=\"#64748B\"\n");
                        dot.push_str("    ];\n");
                    }
                }

                if let Some(sid) = &mapping.job_id {
                    if let Some(src_job) = self.lab.jobs.get(sid) {
                        let src_name = src_job.name.clone().unwrap_or_default();
                        let clean_src = clean_id(&src_name);

                        if clean_src == clean_tgt {
                            continue;
                        }

                        let src_run = job_to_run.get(sid);
                        let tgt_run = self.lab.jobs.iter().find_map(|(jid, j)| {
                            if j.name.as_deref().map(clean_id).as_deref() == Some(&clean_tgt) {
                                job_to_run.get(jid)
                            } else {
                                None
                            }
                        });

                        if src_run != tgt_run {
                            let dtype = "hard".to_string();
                            let key = (clean_src.clone(), clean_tgt.clone(), dtype.clone());
                            if drawn.contains(&key) {
                                continue;
                            }
                            drawn.insert(key);

                            let src_is_sg = self.scatter_gather_clean_names.contains(&clean_src);
                            let dst_is_sg = self.scatter_gather_clean_names.contains(&clean_tgt);

                            let actual_src = if src_is_sg {
                                format!("pipe_{}_sg_gather", clean_src)
                            } else {
                                format!("pipe_{}", clean_src)
                            };
                            let actual_dst = if dst_is_sg {
                                format!("pipe_{}_sg_scatter", clean_tgt)
                            } else {
                                format!("pipe_{}", clean_tgt)
                            };

                            dot_writeln!(dot, "    {} -> {} [", actual_src, actual_dst);
                            if src_is_sg {
                                dot_writeln!(
                                    dot,
                                    "        ltail=\"cluster_pipe_{}_sg\",",
                                    clean_src
                                );
                            }
                            if dst_is_sg {
                                dot_writeln!(
                                    dot,
                                    "        lhead=\"cluster_pipe_{}_sg\",",
                                    clean_tgt
                                );
                            }
                            dot.push_str("        style=\"solid\",\n");
                            dot.push_str("        color=\"#64748B\"\n");
                            dot.push_str("    ];\n");
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::expect_used)]
    fn render_scatter_gather_subgraph(
        &self,
        dot: &mut String,
        job_name: &str,
        unique_node_id: &str,
        count: usize,
        representative_job: &Job,
        indent: &str,
    ) {
        let cluster_id = format!("{}_sg", unique_node_id);
        let scatter_id = format!("{}_sg_scatter", unique_node_id);
        let gather_id = format!("{}_sg_gather", unique_node_id);

        let mut step_names: Vec<String> = representative_job
            .executables
            .keys()
            .filter(|k| k.starts_with("step-"))
            .map(|k| {
                k.strip_prefix("step-")
                    .expect("prefix guaranteed by starts_with filter")
                    .to_string()
            })
            .collect();
        step_names.sort();

        dot_writeln!(dot, "{}subgraph cluster_{} {{", indent, cluster_id);
        dot_writeln!(
            dot,
            "{}    label=\"{}\\n(x{})\";",
            indent,
            escape_dot_label(job_name),
            count
        );
        dot_writeln!(dot, "{}    style=\"filled,rounded,bold\";", indent);
        dot_writeln!(dot, "{}    color=\"{}\";", indent, SG_CLUSTER_BORDER);
        dot_writeln!(dot, "{}    fillcolor=\"{}\";", indent, SG_CLUSTER_BG);
        dot_writeln!(dot, "{}    fontsize=\"{}\";", indent, JOB_FONT_SIZE);
        dot_writeln!(dot, "{}    penwidth=\"1.5\";", indent);
        dot_writeln!(dot, "{}    margin=\"12\";", indent);
        dot.push('\n');

        dot_writeln!(dot, "{}    {} [", indent, scatter_id);
        dot_writeln!(dot, "{}        label=\"scatter\",", indent);
        dot_writeln!(dot, "{}        shape=\"trapezium\",", indent);
        dot_writeln!(dot, "{}        style=\"filled\",", indent);
        dot_writeln!(dot, "{}        fillcolor=\"{}\",", indent, SG_SCATTER_FILL);
        dot_writeln!(dot, "{}        color=\"{}\",", indent, SG_CLUSTER_BORDER);
        dot_writeln!(
            dot,
            "{}        fontsize=\"{}\",",
            indent,
            SG_PHASE_FONT_SIZE
        );
        dot_writeln!(dot, "{}        penwidth=\"1\"", indent);
        dot_writeln!(dot, "{}    ];", indent);

        for step_name in &step_names {
            let step_id = format!("{}_sg_step_{}", unique_node_id, clean_id(step_name));
            dot_writeln!(dot, "{}    {} [", indent, step_id);
            dot_writeln!(
                dot,
                "{}        label=\"{}\",",
                indent,
                escape_dot_label(step_name)
            );
            dot_writeln!(dot, "{}        shape=\"box\",", indent);
            dot_writeln!(dot, "{}        style=\"filled,rounded\",", indent);
            dot_writeln!(dot, "{}        fillcolor=\"{}\",", indent, SG_STEP_FILL);
            dot_writeln!(dot, "{}        color=\"{}\",", indent, SG_STEP_BORDER);
            dot_writeln!(dot, "{}        fontsize=\"{}\",", indent, SG_STEP_FONT_SIZE);
            dot_writeln!(dot, "{}        penwidth=\"1\"", indent);
            dot_writeln!(dot, "{}    ];", indent);
        }

        dot_writeln!(dot, "{}    {} [", indent, gather_id);
        dot_writeln!(dot, "{}        label=\"gather\",", indent);
        dot_writeln!(dot, "{}        shape=\"invtrapezium\",", indent);
        dot_writeln!(dot, "{}        style=\"filled\",", indent);
        dot_writeln!(dot, "{}        fillcolor=\"{}\",", indent, SG_GATHER_FILL);
        dot_writeln!(dot, "{}        color=\"{}\",", indent, SG_CLUSTER_BORDER);
        dot_writeln!(
            dot,
            "{}        fontsize=\"{}\",",
            indent,
            SG_PHASE_FONT_SIZE
        );
        dot_writeln!(dot, "{}        penwidth=\"1\"", indent);
        dot_writeln!(dot, "{}    ];", indent);
        dot.push('\n');

        let all_deps: HashSet<String> = step_names
            .iter()
            .filter_map(|name| {
                let exe_key = format!("step-{}", name);
                representative_job.executables.get(&exe_key)
            })
            .flat_map(|exe| exe.deps.iter().cloned())
            .collect();

        let root_steps: Vec<&String> = step_names
            .iter()
            .filter(|name| {
                let exe_key = format!("step-{}", name);
                representative_job
                    .executables
                    .get(&exe_key)
                    .map(|e| e.deps.is_empty())
                    .unwrap_or(true)
            })
            .collect();

        let sink_steps: Vec<&String> = step_names
            .iter()
            .filter(|name| !all_deps.contains(*name))
            .collect();

        for root in &root_steps {
            let step_id = format!("{}_sg_step_{}", unique_node_id, clean_id(root));
            dot_writeln!(
                dot,
                "{}    {} -> {} [color=\"{}\", penwidth=\"1.0\", arrowsize=\"0.6\"];",
                indent,
                scatter_id,
                step_id,
                SG_INTERNAL_EDGE_COLOR
            );
        }

        for step_name in &step_names {
            let exe_key = format!("step-{}", step_name);
            if let Some(exe) = representative_job.executables.get(&exe_key) {
                let step_id = format!("{}_sg_step_{}", unique_node_id, clean_id(step_name));
                for dep_name in &exe.deps {
                    let dep_id = format!("{}_sg_step_{}", unique_node_id, clean_id(dep_name));
                    dot_writeln!(
                        dot,
                        "{}    {} -> {} [color=\"{}\", penwidth=\"1.0\", arrowsize=\"0.6\"];",
                        indent,
                        dep_id,
                        step_id,
                        SG_INTERNAL_EDGE_COLOR
                    );
                }
            }
        }

        for sink in &sink_steps {
            let step_id = format!("{}_sg_step_{}", unique_node_id, clean_id(sink));
            dot_writeln!(
                dot,
                "{}    {} -> {} [color=\"{}\", penwidth=\"1.0\", arrowsize=\"0.6\"];",
                indent,
                step_id,
                gather_id,
                SG_INTERNAL_EDGE_COLOR
            );
        }

        dot_writeln!(dot, "{}}}", indent);
    }

    fn get_job_inputs(job: &'a Job) -> Vec<&'a repx_core::model::InputMapping> {
        match job.stage_type {
            StageType::Simple => job
                .executables
                .get("main")
                .map(|e| e.inputs.iter().collect())
                .unwrap_or_default(),
            StageType::ScatterGather => job
                .executables
                .get("scatter")
                .map(|e| e.inputs.iter().collect())
                .unwrap_or_default(),
            StageType::Worker | StageType::Gather => Vec::new(),
        }
    }

    fn get_varying_params(&self, job_ids: &[&JobId]) -> BTreeMap<String, Vec<Value>> {
        if job_ids.is_empty() {
            return BTreeMap::new();
        }

        let mut all_keys = HashSet::new();
        for jid in job_ids {
            if let Some(job) = self.lab.jobs.get(jid) {
                if let Value::Object(params) = &job.params {
                    for k in params.keys() {
                        all_keys.insert(k.clone());
                    }
                }
            }
        }

        let mut varying = BTreeMap::new();
        let missing_marker = Value::String("?".to_string());

        for key in all_keys {
            let mut values = HashSet::new();
            let mut has_values = false;

            for jid in job_ids {
                let val = self
                    .lab
                    .jobs
                    .get(jid)
                    .and_then(|job| job.params.get(&key))
                    .unwrap_or(&missing_marker);

                let s_val = canonical_json(val);
                values.insert(s_val);
                if val != &missing_marker {
                    has_values = true;
                }
            }

            if has_values && values.len() > 1 {
                let mut clean_values: Vec<String> =
                    values.into_iter().filter(|v| v != "?").collect();
                clean_values.sort();

                varying.insert(key, clean_values.into_iter().map(Value::String).collect());
            }
        }
        varying
    }
}
