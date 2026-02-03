use anyhow::{Context, Result};
use regex::Regex;
use repx_core::model::{Job, JobId, Lab};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const DPI: &str = "300";
const FONT_NAME: &str = "Helvetica, Arial, sans-serif";
const RANK_SEP: &str = "0.6";
const NODE_SEP: &str = "0.4";
const JOB_FONT_SIZE: &str = "12";
const COLOR_CLUSTER_BORDER: &str = "#334155";
const PARAM_SHAPE: &str = "note";
const PARAM_FILL: &str = "#FFFFFF";
const PARAM_BORDER: &str = "#94a3b8";
const PARAM_FONT_COLOR: &str = "#475569";
const PARAM_FONT_SIZE: &str = "9";
const PARAM_MAX_WIDTH: usize = 20;

lazy_static::lazy_static! {
    static ref PALETTE: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("producer", "#EFF6FF");
        m.insert("consumer", "#ECFDF5");
        m.insert("worker", "#ECFDF5");
        m.insert("partial", "#FFFBEB");
        m.insert("total", "#FFF1F2");
        m.insert("default", "#F8FAFC");
        m
    };
}

#[derive(Debug, Clone)]
pub struct VizArgs {
    pub lab: PathBuf,
    pub output: Option<PathBuf>,
    pub format: Option<String>,
}

pub fn run(args: VizArgs) -> Result<()> {
    let lab = repx_core::lab::load_from_path(&args.lab)?;

    let mut generator = VizGenerator::new(&lab);
    let dot_content = generator.generate_dot(&args)?;

    let output_base = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from("topology"));
    let format = args.format.unwrap_or_else(|| "png".to_string());

    let dot_path = if let Some(parent) = output_base.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        output_base.with_extension("dot")
    } else {
        PathBuf::from("topology.dot")
    };

    fs::write(&dot_path, dot_content)?;

    println!("Rendering {}.{}...", output_base.display(), format);

    let output_file = output_base.with_extension(&format);

    let status = Command::new("dot")
        .arg(format!("-T{}", format))
        .arg(&dot_path)
        .arg("-o")
        .arg(&output_file)
        .status()
        .context("Failed to execute 'dot'. Is Graphviz installed?")?;

    if !status.success() {
        anyhow::bail!("Graphviz exited with error");
    }

    let _ = fs::remove_file(dot_path);

    println!("Done.");
    Ok(())
}

struct VizGenerator<'a> {
    lab: &'a Lab,
    effective_params_cache: HashMap<JobId, Value>,
}

impl<'a> VizGenerator<'a> {
    fn new(lab: &'a Lab) -> Self {
        Self {
            lab,
            effective_params_cache: HashMap::new(),
        }
    }

    fn generate_dot(&mut self, args: &VizArgs) -> Result<String> {
        self.compute_all_effective_params()?;

        let mut dot = String::new();
        dot.push_str("digraph \"RepX Topology\" {\n");

        if args.format.as_deref() != Some("svg") {
            dot.push_str(&format!("    dpi=\"{}\";\n", DPI));
        }
        dot.push_str("    compound=\"true\";\n");
        dot.push_str("    rankdir=\"LR\";\n");
        dot.push_str("    bgcolor=\"#FFFFFF\";\n");
        dot.push_str(&format!("    nodesep=\"{}\";\n", NODE_SEP));
        dot.push_str(&format!("    ranksep=\"{}\";\n", RANK_SEP));
        dot.push_str(&format!("    node [fontname=\"{}\"];\n", FONT_NAME));
        dot.push_str("    edge [color=\"#000000\", penwidth=\"1.2\", arrowsize=\"0.7\"];\n\n");

        let mut grouped_jobs: BTreeMap<String, BTreeMap<String, Vec<&JobId>>> = BTreeMap::new();
        let mut run_anchors: HashMap<String, String> = HashMap::new();

        let mut job_to_run: HashMap<JobId, String> = HashMap::new();
        for (run_id, run) in &self.lab.runs {
            for jid in &run.jobs {
                job_to_run.insert(jid.clone(), run_id.0.clone());
            }
        }

        for (jid, job) in &self.lab.jobs {
            let run_name = job_to_run
                .get(jid)
                .cloned()
                .unwrap_or_else(|| "detached".to_string());
            let job_name = job.name.clone().unwrap_or_else(|| jid.0.clone());

            grouped_jobs
                .entry(run_name.clone())
                .or_default()
                .entry(job_name.clone())
                .or_default()
                .push(jid);

            run_anchors.entry(run_name).or_insert(job_name);
        }

        let mut intra_edges: HashMap<(String, String), usize> = HashMap::new();
        let mut inter_edges: HashSet<(String, String, String)> = HashSet::new();

        for (jid, job) in &self.lab.jobs {
            let run_name = job_to_run
                .get(jid)
                .cloned()
                .unwrap_or_else(|| "detached".to_string());
            let clean_run = clean_id(&run_name);

            let tgt_name = job.name.clone().unwrap_or_else(|| jid.0.clone());
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
                        let src_name = src_job.name.clone().unwrap_or_else(|| sid.0.clone());
                        let clean_src_run = clean_id(&src_run);
                        let clean_src = clean_id(&src_name);
                        let unique_src = format!("{}_{}", clean_src_run, clean_src);

                        *intra_edges
                            .entry((unique_src, unique_tgt.clone()))
                            .or_default() += 1;
                    }
                }

                if let Some(srun) = &mapping.source_run {
                    let dtype = mapping
                        .dependency_type
                        .clone()
                        .unwrap_or_else(|| "hard".to_string());
                    inter_edges.insert((srun.0.clone(), unique_tgt.clone(), dtype));
                }
            }
        }

        for (run_name, groups) in grouped_jobs {
            let clean_run = clean_id(&run_name);
            dot.push_str(&format!("    subgraph cluster_{} {{\n", clean_run));
            dot.push_str(&format!("        label=\"Run: {}\";\n", run_name));
            dot.push_str("        style=\"dashed,rounded\";\n");
            dot.push_str(&format!("        color=\"{}\";\n", COLOR_CLUSTER_BORDER));
            dot.push_str("        fontsize=\"14\";\n");
            dot.push_str("        margin=\"16\";\n");

            for (group_name, job_ids) in groups {
                let count = job_ids.len();
                let job_label = format!("{}\\n(x{})", group_name, count);
                let fill_color = get_fill_color(&group_name);
                let clean_group = clean_id(&group_name);
                let unique_node_id = format!("{}_{}", clean_run, clean_group);

                dot.push_str(&format!("        {} [\n", unique_node_id));
                dot.push_str(&format!("            label=\"{}\",\n", job_label));
                dot.push_str("            shape=\"box\",\n");
                dot.push_str("            style=\"filled,rounded\",\n");
                dot.push_str(&format!("            fontsize=\"{}\",\n", JOB_FONT_SIZE));
                dot.push_str(&format!("            fillcolor=\"{}\",\n", fill_color));
                dot.push_str("            penwidth=\"1\"\n");
                dot.push_str("        ];\n");

                let varying_params = self.get_varying_params(&job_ids);
                for (p_key, p_vals) in varying_params {
                    let clean_key = clean_id(&p_key);

                    let param_node_id = format!("p_{}_{}_{}", clean_run, clean_group, clean_key);

                    let clean_vals: Vec<String> = p_vals
                        .iter()
                        .map(|v| smart_truncate(v, PARAM_MAX_WIDTH))
                        .collect();

                    let mut val_str = clean_vals.join(", ");
                    if val_str.len() > PARAM_MAX_WIDTH {
                        let keep = PARAM_MAX_WIDTH.saturating_sub(2);
                        val_str = format!("{}..", &val_str[..keep]);
                    }

                    let label = format!("{}:\\n{}", p_key, val_str);

                    dot.push_str(&format!("        {} [\n", param_node_id));
                    dot.push_str(&format!("            label=\"{}\",\n", label));
                    dot.push_str(&format!("            shape=\"{}\",\n", PARAM_SHAPE));
                    dot.push_str("            style=\"filled\",\n");
                    dot.push_str(&format!("            fillcolor=\"{}\",\n", PARAM_FILL));
                    dot.push_str(&format!("            color=\"{}\",\n", PARAM_BORDER));
                    dot.push_str(&format!(
                        "            fontcolor=\"{}\",\n",
                        PARAM_FONT_COLOR
                    ));
                    dot.push_str(&format!("            fontsize=\"{}\",\n", PARAM_FONT_SIZE));
                    dot.push_str("            margin=\"0.1,0.05\",\n");
                    dot.push_str("            penwidth=\"0.8\"\n");
                    dot.push_str("        ];\n");

                    dot.push_str(&format!(
                        "        {} -> {} [\n",
                        param_node_id, unique_node_id
                    ));
                    dot.push_str("            style=\"dotted\",\n");
                    dot.push_str(&format!("            color=\"{}\",\n", PARAM_BORDER));
                    dot.push_str("            arrowhead=\"dot\",\n");
                    dot.push_str("            arrowsize=\"0.5\",\n");
                    dot.push_str("            penwidth=\"1.0\"\n");
                    dot.push_str("        ];\n");
                }
            }
            dot.push_str("    }\n");
        }

        for ((src, dst), cnt) in intra_edges {
            let width = if cnt > 1 { "2.0" } else { "1.2" };
            dot.push_str(&format!(
                "    {} -> {} [penwidth=\"{}\"];\n",
                src, dst, width
            ));
        }

        let mut sorted_inter: Vec<_> = inter_edges.into_iter().collect();
        sorted_inter.sort();

        for (srun, unique_tgt, dtype) in sorted_inter {
            if let Some(anchor_job) = run_anchors.get(&srun) {
                let clean_srun = clean_id(&srun);
                let clean_anchor = clean_id(anchor_job);
                let unique_anchor = format!("{}_{}", clean_srun, clean_anchor);

                let style = if dtype == "soft" { "dashed" } else { "solid" };
                dot.push_str(&format!("    {} -> {} [\n", unique_anchor, unique_tgt));
                dot.push_str(&format!("        ltail=\"cluster_{}\",\n", clean_srun));
                dot.push_str(&format!("        style=\"{}\",\n", style));
                dot.push_str("        color=\"#64748B\"\n");
                dot.push_str("    ];\n");
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    fn get_job_inputs(job: &'a Job) -> Vec<&'a repx_core::model::InputMapping> {
        let stage_type = &job.stage_type;
        if stage_type == "simple" {
            job.executables
                .get("main")
                .map(|e| e.inputs.iter().collect())
                .unwrap_or_default()
        } else if stage_type == "scatter-gather" {
            job.executables
                .get("scatter")
                .map(|e| e.inputs.iter().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    fn compute_all_effective_params(&mut self) -> Result<()> {
        let job_ids: Vec<JobId> = self.lab.jobs.keys().cloned().collect();
        for jid in job_ids {
            self.get_single_effective_params(&jid, &mut HashSet::new())?;
        }
        Ok(())
    }

    fn get_single_effective_params(
        &mut self,
        job_id: &JobId,
        visiting: &mut HashSet<JobId>,
    ) -> Result<Value> {
        if let Some(v) = self.effective_params_cache.get(job_id) {
            return Ok(v.clone());
        }

        if visiting.contains(job_id) {
            anyhow::bail!("Circular dependency detected at {}", job_id.0);
        }
        visiting.insert(job_id.clone());

        let job = self
            .lab
            .jobs
            .get(job_id)
            .ok_or_else(|| anyhow::anyhow!("Job ID {} not found", job_id.0))?;

        let mut effective_params = serde_json::Map::new();

        let inputs = Self::get_job_inputs(job);
        for mapping in inputs {
            if let Some(dep_id) = &mapping.job_id {
                if let Ok(Value::Object(map)) = self.get_single_effective_params(dep_id, visiting) {
                    for (k, v) in map {
                        effective_params.insert(k, v);
                    }
                }
            }
        }

        if let Value::Object(map) = &job.params {
            for (k, v) in map {
                effective_params.insert(k.clone(), v.clone());
            }
        }

        visiting.remove(job_id);
        let result = Value::Object(effective_params);
        self.effective_params_cache
            .insert(job_id.clone(), result.clone());
        Ok(result)
    }

    fn get_varying_params(&self, job_ids: &[&JobId]) -> BTreeMap<String, Vec<Value>> {
        if job_ids.is_empty() {
            return BTreeMap::new();
        }

        let mut all_keys = HashSet::new();
        for jid in job_ids {
            if let Some(Value::Object(params)) = self.effective_params_cache.get(jid) {
                for k in params.keys() {
                    all_keys.insert(k.clone());
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
                    .effective_params_cache
                    .get(jid)
                    .and_then(|p| p.get(&key))
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

fn canonical_json(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}

fn get_fill_color(name: &str) -> String {
    let name_lower = name.to_lowercase();
    for (key, color) in PALETTE.iter() {
        if name_lower.contains(key) {
            return color.to_string();
        }
    }
    PALETTE.get("default").unwrap().to_string()
}

fn clean_id(s: &str) -> String {
    let re = Regex::new(r"[^a-zA-Z0-9_]").unwrap();
    re.replace_all(s, "").to_string()
}

fn smart_truncate(val: &Value, max_len: usize) -> String {
    let mut s = match val {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    };

    if s.contains('/') {
        if let Some(filename) = s.split('/').next_back() {
            s = filename.to_string();
        }
    }

    s = s.replace(['[', ']', '\'', '"'], "");

    if s.len() > max_len {
        let keep = (max_len / 2).saturating_sub(2);
        let chars: Vec<char> = s.chars().collect();
        if chars.len() > max_len {
            let start: String = chars.iter().take(keep).collect();
            let end: String = chars.iter().rev().take(keep).collect();
            let end_correct: String = end.chars().rev().collect();
            return format!("{}..{}", start, end_correct);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_fill_color() {
        assert_eq!(
            get_fill_color("stage-producer-abc"),
            *PALETTE.get("producer").unwrap()
        );
        assert_eq!(
            get_fill_color("stage-consumer-xyz"),
            *PALETTE.get("consumer").unwrap()
        );
        assert_eq!(
            get_fill_color("data-worker-123"),
            *PALETTE.get("worker").unwrap()
        );
        assert_eq!(
            get_fill_color("partial-sum-stage"),
            *PALETTE.get("partial").unwrap()
        );
        assert_eq!(
            get_fill_color("total-sum-stage"),
            *PALETTE.get("total").unwrap()
        );
        assert_eq!(
            get_fill_color("random-stage-name"),
            *PALETTE.get("default").unwrap()
        );

        assert_eq!(
            get_fill_color("STAGE-PRODUCER"),
            *PALETTE.get("producer").unwrap()
        );
        assert_eq!(
            get_fill_color("Stage-Consumer"),
            *PALETTE.get("consumer").unwrap()
        );

        assert_eq!(get_fill_color(""), *PALETTE.get("default").unwrap());
    }

    #[test]
    fn test_clean_id() {
        assert_eq!(clean_id("stage-A-producer"), "stageAproducer");
        assert_eq!(clean_id("job@123#test"), "job123test");
        assert_eq!(clean_id("valid_name_123"), "valid_name_123");
        assert_eq!(clean_id(""), "");
        assert_eq!(clean_id("@#$%^&*"), "");
        assert_eq!(clean_id("name"), "name");
    }

    #[test]
    fn test_smart_truncate() {
        let short = Value::String("short".to_string());
        assert_eq!(smart_truncate(&short, 30), "short");

        let long_str = Value::String("a".repeat(50));
        let result = smart_truncate(&long_str, 20);
        assert!(result.len() <= 20);
        assert!(result.contains(".."));

        let path = Value::String("/very/long/path/to/filename.txt".to_string());
        assert_eq!(smart_truncate(&path, 30), "filename.txt");

        let arr = Value::String("[1, 2, 3]".to_string());
        let res = smart_truncate(&arr, 30);
        assert!(!res.contains('['));
        assert!(!res.contains(']'));

        let quoted = Value::String("'quoted'".to_string());
        let res = smart_truncate(&quoted, 30);
        assert!(!res.contains('\''));

        let num = serde_json::json!(12345);
        assert_eq!(smart_truncate(&num, 30), "12345");

        let exact = Value::String("x".repeat(10));
        assert_eq!(smart_truncate(&exact, 10), "xxxxxxxxxx");

        let boundary = Value::String("a".repeat(11));
        let res = smart_truncate(&boundary, 10);
        assert!(res.len() <= 10);
    }
}
