pub fn tree_prefix(
    ancestor_is_last: &[bool],
    depth: usize,
    is_last_child: bool,
    marker: &str,
) -> String {
    if depth == 0 {
        return format!("{} ", marker);
    }

    let mut buf = String::with_capacity(2 * depth + marker.len() + 4);
    buf.push(' ');

    for &is_last in &ancestor_is_last[1..depth] {
        if is_last {
            buf.push_str("  ");
        } else {
            buf.push_str("│ ");
        }
    }

    if is_last_child {
        buf.push('└');
    } else {
        buf.push('├');
    }

    buf.push_str(marker);
    buf.push(' ');
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_tree(nodes: &[(usize, bool, &str, &str)]) -> Vec<String> {
        let mut ancestor_is_last: Vec<bool> = Vec::new();
        let mut lines = Vec::new();

        for &(depth, is_last, marker, label) in nodes {
            while ancestor_is_last.len() > depth {
                ancestor_is_last.pop();
            }
            let prefix = tree_prefix(&ancestor_is_last, depth, is_last, marker);
            lines.push(format!("{}{}", prefix, label));
            ancestor_is_last.push(is_last);
        }
        lines
    }

    #[test]
    fn test_ungrouped_single_run_single_job() {
        let lines = render_tree(&[
            (0, true, "[-]", "simulation-run"),
            (1, true, "───", "stage-A"),
        ]);
        assert_eq!(lines, vec!["[-] simulation-run", " └─── stage-A",]);
    }

    #[test]
    fn test_ungrouped_single_run_two_jobs() {
        let lines = render_tree(&[
            (0, true, "[-]", "simulation-run"),
            (1, false, "[-]", "stage-B"),
            (1, true, "───", "stage-A"),
        ]);
        assert_eq!(
            lines,
            vec!["[-] simulation-run", " ├[-] stage-B", " └─── stage-A",]
        );
    }

    #[test]
    fn test_ungrouped_two_runs_with_jobs() {
        let lines = render_tree(&[
            (0, false, "[-]", "run-1"),
            (1, false, "[-]", "job-A"),
            (1, true, "───", "job-B"),
            (0, true, "[-]", "run-2"),
            (1, true, "───", "job-C"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] run-1",
                " ├[-] job-A",
                " └─── job-B",
                "[-] run-2",
                " └─── job-C",
            ]
        );
    }

    #[test]
    fn test_ungrouped_deep_chain() {
        let lines = render_tree(&[
            (0, true, "[-]", "run-1"),
            (1, true, "[-]", "stage-F"),
            (2, true, "[-]", "stage-E"),
            (3, true, "[-]", "stage-D"),
            (4, true, "───", "stage-A"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] run-1",
                " └[-] stage-F",
                "   └[-] stage-E",
                "     └[-] stage-D",
                "       └─── stage-A",
            ]
        );
    }

    #[test]
    fn test_ungrouped_branching() {
        let lines = render_tree(&[
            (0, true, "[-]", "run-1"),
            (1, false, "[-]", "stage-F"),
            (2, true, "[-]", "stage-E"),
            (3, true, "───", "stage-D"),
            (1, true, "───", "stage-G"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] run-1",
                " ├[-] stage-F",
                " │ └[-] stage-E",
                " │   └─── stage-D",
                " └─── stage-G",
            ]
        );
    }

    #[test]
    fn test_grouped_single_group_single_run_single_job() {
        let lines = render_tree(&[
            (0, true, "[-]", "@all"),
            (1, true, "[-]", "run-1"),
            (2, true, "───", "stage-A"),
        ]);
        assert_eq!(lines, vec!["[-] @all", " └[-] run-1", "   └─── stage-A",]);
    }

    #[test]
    fn test_grouped_single_group_two_runs() {
        let lines = render_tree(&[
            (0, true, "[-]", "@all"),
            (1, false, "[-]", "simulation-run"),
            (2, false, "[-]", "stage-F"),
            (2, true, "───", "stage-G"),
            (1, true, "[-]", "analysis-run"),
            (2, true, "───", "stage-H"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] @all",
                " ├[-] simulation-run",
                " │ ├[-] stage-F",
                " │ └─── stage-G",
                " └[-] analysis-run",
                "   └─── stage-H",
            ]
        );
    }

    #[test]
    fn test_grouped_deep_chain() {
        let lines = render_tree(&[
            (0, true, "[-]", "@all"),
            (1, true, "[-]", "simulation-run"),
            (2, true, "[-]", "stage-F"),
            (3, true, "[-]", "stage-E"),
            (4, true, "[-]", "stage-D"),
            (5, true, "[-]", "stage-C"),
            (6, false, "───", "stage-A"),
            (6, true, "───", "stage-B"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] @all",
                " └[-] simulation-run",
                "   └[-] stage-F",
                "     └[-] stage-E",
                "       └[-] stage-D",
                "         └[-] stage-C",
                "           ├─── stage-A",
                "           └─── stage-B",
            ]
        );
    }

    #[test]
    fn test_grouped_two_groups() {
        let lines = render_tree(&[
            (0, false, "[-]", "@all"),
            (1, false, "[-]", "sim-run"),
            (2, false, "[-]", "stage-F-fast"),
            (2, true, "[-]", "stage-F-slow"),
            (1, true, "[-]", "analysis-run"),
            (2, true, "───", "stage-H"),
            (0, true, "[-]", "@compute"),
            (1, true, "[-]", "sim-run"),
            (2, true, "───", "stage-F-fast"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] @all",
                " ├[-] sim-run",
                " │ ├[-] stage-F-fast",
                " │ └[-] stage-F-slow",
                " └[-] analysis-run",
                "   └─── stage-H",
                "[-] @compute",
                " └[-] sim-run",
                "   └─── stage-F-fast",
            ]
        );
    }

    #[test]
    fn test_grouped_non_last_group_deep_jobs() {
        let lines = render_tree(&[
            (0, false, "[-]", "@all"),
            (1, false, "[-]", "sim-run"),
            (2, true, "[-]", "stage-F"),
            (3, true, "[-]", "stage-E"),
            (4, true, "───", "stage-D"),
            (1, true, "[-]", "analysis-run"),
            (2, true, "───", "stage-H"),
            (0, true, "[-]", "@compute"),
            (1, true, "[-]", "sim-run"),
            (2, true, "───", "stage-F"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] @all",
                " ├[-] sim-run",
                " │ └[-] stage-F",
                " │   └[-] stage-E",
                " │     └─── stage-D",
                " └[-] analysis-run",
                "   └─── stage-H",
                "[-] @compute",
                " └[-] sim-run",
                "   └─── stage-F",
            ]
        );
    }

    #[test]
    fn test_grouped_complex_real_world() {
        let lines = render_tree(&[
            (0, true, "[-]", "@all"),
            (1, false, "[-]", "simulation-run"),
            (2, false, "[-]", "stage-F-fast"),
            (3, true, "[-]", "stage-E"),
            (4, true, "[-]", "stage-D"),
            (5, true, "[-]", "stage-C"),
            (6, false, "───", "stage-A"),
            (6, true, "───", "stage-B"),
            (2, true, "[-]", "stage-F-slow"),
            (3, true, "[-]", "stage-E"),
            (4, true, "[-]", "stage-D"),
            (5, true, "[-]", "stage-C"),
            (6, false, "───", "stage-A"),
            (6, true, "───", "stage-B"),
            (1, true, "[-]", "analysis-run"),
            (2, true, "───", "stage-analysis"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] @all",
                " ├[-] simulation-run",
                " │ ├[-] stage-F-fast",
                " │ │ └[-] stage-E",
                " │ │   └[-] stage-D",
                " │ │     └[-] stage-C",
                " │ │       ├─── stage-A",
                " │ │       └─── stage-B",
                " │ └[-] stage-F-slow",
                " │   └[-] stage-E",
                " │     └[-] stage-D",
                " │       └[-] stage-C",
                " │         ├─── stage-A",
                " │         └─── stage-B",
                " └[-] analysis-run",
                "   └─── stage-analysis",
            ]
        );
    }

    #[test]
    fn test_collapsed_marker() {
        let lines = render_tree(&[(0, true, "[+]", "run-1")]);
        assert_eq!(lines, vec!["[+] run-1"]);
    }

    #[test]
    fn test_depth_0_no_branch_chars() {
        let lines = render_tree(&[
            (0, false, "[-]", "root-A"),
            (0, false, "[-]", "root-B"),
            (0, true, "[-]", "root-C"),
        ]);
        assert_eq!(lines, vec!["[-] root-A", "[-] root-B", "[-] root-C",]);
    }

    #[test]
    fn test_ungrouped_two_runs_deep_continuation() {
        let lines = render_tree(&[
            (0, false, "[-]", "run-1"),
            (1, true, "[-]", "stage-F"),
            (2, true, "[-]", "stage-E"),
            (3, true, "───", "stage-D"),
            (0, true, "[-]", "run-2"),
            (1, true, "───", "stage-X"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] run-1",
                " └[-] stage-F",
                "   └[-] stage-E",
                "     └─── stage-D",
                "[-] run-2",
                " └─── stage-X",
            ]
        );
    }

    #[test]
    fn test_ungrouped_multiple_top_jobs_with_children() {
        let lines = render_tree(&[
            (0, true, "[-]", "run-1"),
            (1, false, "───", "job-A"),
            (1, false, "[-]", "job-B"),
            (2, true, "───", "job-B-child"),
            (1, true, "───", "job-C"),
        ]);
        assert_eq!(
            lines,
            vec![
                "[-] run-1",
                " ├─── job-A",
                " ├[-] job-B",
                " │ └─── job-B-child",
                " └─── job-C",
            ]
        );
    }
}
