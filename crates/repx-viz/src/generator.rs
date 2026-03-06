use anyhow::Result;
use repx_core::model::{Job, JobId, Lab, StageType};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::helpers::*;
use crate::VizArgs;

type StageJobs<'a> = BTreeMap<String, Vec<&'a JobId>>;

type RunStages<'a> = BTreeMap<String, StageJobs<'a>>;

pub(crate) struct VizGenerator<'a> {
    pub lab: &'a Lab,
}

impl<'a> VizGenerator<'a> {
    pub fn new(lab: &'a Lab) -> Self {
        Self { lab }
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
        dot_writeln!(dot, "    nodesep=\"{}\";", NODE_SEP);
        dot_writeln!(dot, "    ranksep=\"{}\";", RANK_SEP);
        dot_writeln!(dot, "    node [fontname=\"{}\"];", FONT_NAME);
        dot.push_str("    edge [color=\"#000000\", penwidth=\"1.2\", arrowsize=\"0.7\"];\n\n");

        let mut grouped_jobs: RunStages<'_> = BTreeMap::new();
        let mut run_anchors: HashMap<String, String> = HashMap::new();

        let mut job_to_run: HashMap<JobId, String> = HashMap::new();
        for (run_id, run) in &self.lab.runs {
            for jid in &run.jobs {
                job_to_run.insert(jid.clone(), run_id.to_string());
            }
        }

        for (jid, job) in &self.lab.jobs {
            let run_name = job_to_run
                .get(jid)
                .cloned()
                .unwrap_or_else(|| "detached".to_string());
            let job_name = job.name.clone().unwrap_or_else(|| jid.to_string());

            grouped_jobs
                .entry(run_name.clone())
                .or_default()
                .entry(job_name.clone())
                .or_default()
                .push(jid);

            run_anchors.entry(run_name).or_insert(job_name);
        }

        let mut intra_edges: HashMap<(String, String, String, String), usize> = HashMap::new();
        let mut inter_edges: HashSet<(String, String, String)> = HashSet::new();

        for (jid, job) in &self.lab.jobs {
            let run_name = job_to_run
                .get(jid)
                .cloned()
                .unwrap_or_else(|| "detached".to_string());
            let clean_run = clean_id(&run_name);

            let tgt_name = job.name.clone().unwrap_or_else(|| jid.to_string());
            let clean_tgt = clean_id(&tgt_name);
            let unique_tgt = format!("{}_{}", clean_run, clean_tgt);

            let inputs = Self::get_job_inputs(job);

            for mapping in inputs {
                if let Some(sid) = &mapping.job_id {
                    let src_run = job_to_run
                        .get(sid)
                        .cloned()
                        .unwrap_or_else(|| "detached".to_string());

                    if let Some(src_job) = self.lab.jobs.get(sid) {
                        let src_name = src_job.name.clone().unwrap_or_else(|| sid.to_string());
                        let clean_src_run = clean_id(&src_run);
                        let clean_src = clean_id(&src_name);

                        *intra_edges
                            .entry((
                                clean_src_run,
                                clean_src,
                                clean_run.clone(),
                                clean_tgt.clone(),
                            ))
                            .or_default() += 1;
                    }
                }

                if let Some(srun) = &mapping.source_run {
                    let dtype = mapping
                        .dependency_type
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "hard".to_string());
                    inter_edges.insert((srun.to_string(), unique_tgt.clone(), dtype));
                }
            }
        }

        let has_groups = !self.lab.groups.is_empty();
        let mut runs_in_groups: HashSet<String> = HashSet::new();

        let mut node_prefixes: Vec<String> = Vec::new();

        if has_groups {
            let mut sorted_groups: Vec<_> = self.lab.groups.iter().collect();
            sorted_groups.sort_by_key(|(name, _)| *name);

            for (group_name, group_run_ids) in sorted_groups {
                if group_run_ids.is_empty() {
                    continue;
                }

                let clean_group = clean_id(group_name);
                dot_writeln!(dot, "    subgraph cluster_group_{} {{", clean_group);
                dot_writeln!(dot, "        label=\"@{}\";", escape_dot_label(group_name));
                dot.push_str("        style=\"solid,rounded\";\n");
                dot_writeln!(dot, "        color=\"{}\";", COLOR_GROUP_BORDER);
                dot_writeln!(dot, "        fontsize=\"{}\";", GROUP_FONT_SIZE);
                dot.push_str("        penwidth=\"2\";\n");
                dot.push_str("        margin=\"20\";\n\n");

                for run_id in group_run_ids {
                    let run_name = run_id.as_str();
                    runs_in_groups.insert(run_name.to_string());
                    if let Some(jobs) = grouped_jobs.get(run_name) {
                        let prefix = format!("{}_{}", clean_group, clean_id(run_name));
                        node_prefixes.push(prefix.clone());
                        self.render_run_cluster(&mut dot, run_name, &prefix, jobs, "        ");
                    }
                }

                dot.push_str("    }\n\n");
            }
        }

        for (run_name, jobs) in &grouped_jobs {
            if has_groups && runs_in_groups.contains(run_name) {
                continue;
            }
            let prefix = clean_id(run_name);
            node_prefixes.push(prefix.clone());
            self.render_run_cluster(&mut dot, run_name, &prefix, jobs, "    ");
        }

        for prefix in &node_prefixes {
            for ((src_run, src_job_name, dst_run, dst_job_name), cnt) in &intra_edges {
                let prefix_has_src =
                    prefix == src_run || prefix.ends_with(&format!("_{}", src_run));
                let prefix_has_dst =
                    prefix == dst_run || prefix.ends_with(&format!("_{}", dst_run));

                if prefix_has_src && prefix_has_dst {
                    let width = if *cnt > 1 { "2.0" } else { "1.2" };

                    let src_node = format!("{}_{}", prefix, src_job_name);
                    let dst_node = format!("{}_{}", prefix, dst_job_name);

                    let src_is_sg = self.is_scatter_gather_stage_by_clean_name(src_job_name);
                    let dst_is_sg = self.is_scatter_gather_stage_by_clean_name(dst_job_name);

                    let actual_src = if src_is_sg {
                        format!("{}_sg_gather", src_node)
                    } else {
                        src_node.clone()
                    };
                    let actual_dst = if dst_is_sg {
                        format!("{}_sg_scatter", dst_node)
                    } else {
                        dst_node.clone()
                    };

                    dot_write!(
                        dot,
                        "    {} -> {} [penwidth=\"{}\"",
                        actual_src,
                        actual_dst,
                        width
                    );
                    if src_is_sg {
                        dot_write!(dot, ", ltail=\"cluster_{}_sg\"", src_node);
                    }
                    if dst_is_sg {
                        dot_write!(dot, ", lhead=\"cluster_{}_sg\"", dst_node);
                    }
                    dot.push_str("];\n");
                }
            }
        }

        let mut sorted_inter: Vec<_> = inter_edges.into_iter().collect();
        sorted_inter.sort();

        for (srun, unique_tgt, dtype) in sorted_inter {
            if let Some(anchor_job) = run_anchors.get(&srun) {
                let tgt_run = unique_tgt.split('_').next().unwrap_or("");
                let tgt_job = unique_tgt.split('_').next_back().unwrap_or(&unique_tgt);
                let clean_srun = clean_id(&srun);

                for src_prefix in &node_prefixes {
                    if !src_prefix.contains(&clean_srun) {
                        continue;
                    }
                    for tgt_prefix in &node_prefixes {
                        if !tgt_prefix.contains(tgt_run) {
                            continue;
                        }
                        let clean_anchor = clean_id(anchor_job);
                        let unique_anchor = format!("{}_{}", src_prefix, clean_anchor);
                        let prefixed_tgt = format!("{}_{}", tgt_prefix, tgt_job);

                        let style = if dtype == "soft" { "dashed" } else { "solid" };
                        dot_writeln!(dot, "    {} -> {} [", unique_anchor, prefixed_tgt);
                        dot_writeln!(dot, "        ltail=\"cluster_{}\",", src_prefix);
                        dot_writeln!(dot, "        style=\"{}\",", style);
                        dot.push_str("        color=\"#64748B\"\n");
                        dot.push_str("    ];\n");
                    }
                }
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    fn is_scatter_gather_stage_by_clean_name(&self, clean_name: &str) -> bool {
        self.lab.jobs.values().any(|job| {
            let name = job.name.clone().unwrap_or_default();
            clean_id(&name) == clean_name && job.stage_type == StageType::ScatterGather
        })
    }

    #[allow(clippy::expect_used)]
    fn render_run_cluster(
        &self,
        dot: &mut String,
        run_name: &str,
        prefix: &str,
        jobs: &BTreeMap<String, Vec<&JobId>>,
        indent: &str,
    ) {
        dot_writeln!(dot, "{}subgraph cluster_{} {{", indent, prefix);
        dot_writeln!(
            dot,
            "{}    label=\"Run: {}\";",
            indent,
            escape_dot_label(run_name)
        );
        dot_writeln!(dot, "{}    style=\"dashed,rounded\";", indent);
        dot_writeln!(dot, "{}    color=\"{}\";", indent, COLOR_CLUSTER_BORDER);
        dot_writeln!(dot, "{}    fontsize=\"14\";", indent);
        dot_writeln!(dot, "{}    margin=\"16\";", indent);

        for (job_name, job_ids) in jobs {
            let count = job_ids.len();
            let first_job = job_ids.first().and_then(|jid| self.lab.jobs.get(jid));
            let is_sg = first_job
                .map(|j| j.stage_type == StageType::ScatterGather)
                .unwrap_or(false);

            let clean_job = clean_id(job_name);
            let unique_node_id = format!("{}_{}", prefix, clean_job);

            if is_sg {
                self.render_scatter_gather_subgraph(
                    dot,
                    job_name,
                    &unique_node_id,
                    count,
                    first_job.expect("first_job is guaranteed Some when is_sg is true"),
                    &format!("{}    ", indent),
                );
            } else {
                let job_label = format!("{}\\n(x{})", escape_dot_label(job_name), count);
                let fill_color = get_fill_color(job_name);

                dot_writeln!(dot, "{}    {} [", indent, unique_node_id);
                dot_writeln!(dot, "{}        label=\"{}\",", indent, job_label);
                dot_writeln!(dot, "{}        shape=\"box\",", indent);
                dot_writeln!(dot, "{}        style=\"filled,rounded\",", indent);
                dot_writeln!(dot, "{}        fontsize=\"{}\",", indent, JOB_FONT_SIZE);
                dot_writeln!(dot, "{}        fillcolor=\"{}\",", indent, fill_color);
                dot_writeln!(dot, "{}        penwidth=\"1\"", indent);
                dot_writeln!(dot, "{}    ];", indent);
            }

            let varying_params = self.get_varying_params(job_ids);
            for (p_key, p_vals) in varying_params {
                let clean_key = clean_id(&p_key);
                let param_node_id = format!("p_{}_{}_{}", prefix, clean_job, clean_key);

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

                dot_writeln!(dot, "{}    {} [", indent, param_node_id);
                dot_writeln!(dot, "{}        label=\"{}\",", indent, label);
                dot_writeln!(dot, "{}        shape=\"{}\",", indent, PARAM_SHAPE);
                dot_writeln!(dot, "{}        style=\"filled\",", indent);
                dot_writeln!(dot, "{}        fillcolor=\"{}\",", indent, PARAM_FILL);
                dot_writeln!(dot, "{}        color=\"{}\",", indent, PARAM_BORDER);
                dot_writeln!(dot, "{}        fontcolor=\"{}\",", indent, PARAM_FONT_COLOR);
                dot_writeln!(dot, "{}        fontsize=\"{}\",", indent, PARAM_FONT_SIZE);
                dot_writeln!(dot, "{}        margin=\"0.1,0.05\",", indent);
                dot_writeln!(dot, "{}        penwidth=\"0.8\"", indent);
                dot_writeln!(dot, "{}    ];", indent);

                let target_node = if is_sg {
                    format!("{}_sg_scatter", unique_node_id)
                } else {
                    unique_node_id.clone()
                };

                dot_writeln!(dot, "{}    {} -> {} [", indent, param_node_id, target_node);
                dot_writeln!(dot, "{}        style=\"dotted\",", indent);
                dot_writeln!(dot, "{}        color=\"{}\",", indent, PARAM_BORDER);
                dot_writeln!(dot, "{}        arrowhead=\"dot\",", indent);
                dot_writeln!(dot, "{}        arrowsize=\"0.5\",", indent);
                dot_writeln!(dot, "{}        penwidth=\"1.0\"", indent);
                dot_writeln!(dot, "{}    ];", indent);
            }
        }
        dot_writeln!(dot, "{}}}", indent);
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
