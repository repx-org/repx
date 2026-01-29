#!/usr/bin/env python3
import argparse
import collections
import json
import sys
import re
from pathlib import Path
from typing import Any, Dict, List

import graphviz
from .models import Experiment, JobView

DPI = "300"
FONT_NAME = "Helvetica, Arial, sans-serif"

RANK_SEP = "0.6"
NODE_SEP = "0.4"

JOB_FONT_SIZE = "12"
COLOR_CLUSTER_BORDER = "#334155"
PALETTE = {
    "producer": "#EFF6FF",
    "consumer": "#ECFDF5",
    "worker":   "#ECFDF5",
    "partial":  "#FFFBEB",
    "total":    "#FFF1F2",
    "default":  "#F8FAFC"
}

PARAM_SHAPE = "note"
PARAM_FILL = "#FFFFFF"
PARAM_BORDER = "#94a3b8"
PARAM_FONT_COLOR = "#475569"
PARAM_FONT_SIZE = "9"
PARAM_MAX_WIDTH = 20

def get_fill_color(name: str) -> str:
    name_lower = name.lower()
    for key, color in PALETTE.items():
        if key in name_lower:
            return color
    return PALETTE["default"]

def smart_truncate(val: Any, max_len=30) -> str:
    s = str(val)
    if "/" in s:
        s = s.split("/")[-1]

    s = s.replace("[", "").replace("]", "").replace("'", "").replace('"', "")

    if len(s) > max_len:
        keep = int(max_len / 2) - 2
        return s[:keep] + ".." + s[-keep:]
    return s

def get_varying_params(job_list: List[JobView]) -> Dict[str, List[Any]]:
    if not job_list:
        return {}
    all_keys = set()
    for job in job_list:
        all_keys.update(job.params.keys())

    varying_params = {}
    for key in all_keys:
        values = set()
        for job in job_list:
            val = job.params.get(key, "?")

            if isinstance(val, (list, dict)):
                val = json.dumps(val, sort_keys=True)
            values.add(val)
        if len(values) > 0:
             clean_values = [v for v in values if v != "?"]
             if clean_values:
                 varying_params[key] = sorted(list(clean_values))

    return varying_params

def clean_id(s: str) -> str:
    return re.sub(r'[^a-zA-Z0-9_]', '', s)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("lab_path", type=Path)
    parser.add_argument("-o", "--output", type=Path, default=Path("topology"))
    parser.add_argument("--format", type=str, default="png", choices=["png", "pdf", "svg"])
    args = parser.parse_args()

    try:
        exp = Experiment(args.lab_path)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    grouped_jobs = collections.defaultdict(lambda: collections.defaultdict(list))
    run_anchors = {}

    all_jobs = exp.jobs()
    for job in all_jobs:
        try:
            rname, _ = exp.get_run_for_job(job.id)
        except:
            rname = "detached"
        grouped_jobs[rname][job.name].append(job)
        if rname not in run_anchors:
            run_anchors[rname] = job.name

    intra_edges = collections.defaultdict(int)
    inter_edges = []

    for job in all_jobs:
        tgt = job.name
        for m in job.input_mappings:
            sid = m.get("job_id")
            if sid and sid != "self":
                try:
                    src = exp.get_job(sid).name
                    intra_edges[(src, tgt)] += 1
                except: pass

            srun = m.get("source_run")
            if srun:
                dtype = m.get("dependency_type", "hard")
                inter_edges.append((srun, tgt, dtype))

    inter_edges = sorted(list(set(inter_edges)))

    dot = graphviz.Digraph(comment='RepX Topology')

    if args.format != 'svg':
        dot.attr(dpi=DPI)

    dot.attr(compound='true', rankdir='LR', bgcolor="#FFFFFF")
    dot.attr(nodesep=NODE_SEP, ranksep=RANK_SEP)

    dot.attr('node', fontname=FONT_NAME)
    dot.attr('edge', color="#000000", penwidth='1.2', arrowsize='0.7')

    for run_name, groups in grouped_jobs.items():
        with dot.subgraph(name=f"cluster_{run_name}") as c:
            c.attr(label=f"Run: {run_name}",
                   style='dashed,rounded',
                   color=COLOR_CLUSTER_BORDER,
                   fontsize="14",
                   margin="16")

            for group_name, job_list in groups.items():
                count = len(job_list)
                job_label = f"{group_name}\n(x{count})"
                fill_color = get_fill_color(group_name)

                c.node(group_name,
                       label=job_label,
                       shape='box',
                       style='filled,rounded',
                       fontsize=JOB_FONT_SIZE,
                       fillcolor=fill_color,
                       penwidth='1')

                params = get_varying_params(job_list)

                for p_key, p_vals in params.items():

                    clean_key = clean_id(p_key)
                    clean_group = clean_id(group_name)
                    clean_run = clean_id(run_name)

                    param_node_id = f"p_{clean_run}_{clean_group}_{clean_key}"

                    clean_vals = [smart_truncate(v, PARAM_MAX_WIDTH) for v in p_vals]
                    val_str = ", ".join(clean_vals)
                    if len(val_str) > PARAM_MAX_WIDTH:
                        val_str = val_str[:PARAM_MAX_WIDTH-2] + ".."

                    label = f"{p_key}:\n{val_str}"

                    c.node(param_node_id,
                           label=label,
                           shape=PARAM_SHAPE,
                           style='filled',
                           fillcolor=PARAM_FILL,
                           color=PARAM_BORDER,
                           fontcolor=PARAM_FONT_COLOR,
                           fontsize=PARAM_FONT_SIZE,
                           margin="0.1,0.05",
                           penwidth='0.8')

                    c.edge(param_node_id, group_name,
                           style='dotted',
                           color=PARAM_BORDER,
                           arrowhead='dot',
                           arrowsize='0.5',
                           penwidth='1.0')

    for (src, dst), cnt in intra_edges.items():
        w = "2.0" if cnt > 1 else "1.2"
        dot.edge(src, dst, penwidth=w)

    for (srun, tgt, dtype) in inter_edges:
        anchor = run_anchors.get(srun)
        if anchor:
            style = "dashed" if dtype == "soft" else "solid"
            dot.edge(anchor, tgt, ltail=f"cluster_{srun}", style=style, color="#64748B")

    out = str(args.output)
    print(f"Rendering {out}.{args.format}...")
    dot.render(out, format=args.format, cleanup=True)
    print("Done.")

if __name__ == "__main__":
    main()
