use crate::blueprint::RunTemplate;
use serde_json::Value;
use std::collections::BTreeMap;

pub type ParamCombo = BTreeMap<String, Value>;

#[derive(Debug)]
pub enum Axis {
    Single {
        name: String,
        values: Vec<Value>,
    },
    Zip {
        members: Vec<String>,
        rows: Vec<BTreeMap<String, Value>>,
    },
}

impl Axis {
    fn len(&self) -> usize {
        match self {
            Axis::Single { values, .. } => values.len(),
            Axis::Zip { rows, .. } => rows.len(),
        }
    }

    fn write_to(&self, index: usize, combo: &mut ParamCombo) {
        match self {
            Axis::Single { name, values } => {
                combo.insert(name.clone(), values[index].clone());
            }
            Axis::Zip { rows, .. } => {
                for (k, v) in &rows[index] {
                    combo.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

pub fn build_axes(run: &RunTemplate) -> (Vec<Axis>, u128) {
    let mut zip_member_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for zg in &run.zip_groups {
        for m in &zg.members {
            zip_member_names.insert(m.clone());
        }
    }

    let mut axes: Vec<Axis> = Vec::new();

    let mut param_names: Vec<&String> = run.parameter_axes.keys().collect();
    param_names.sort();
    for name in param_names {
        if zip_member_names.contains(name) {
            continue;
        }
        axes.push(Axis::Single {
            name: name.clone(),
            values: run.parameter_axes[name].clone(),
        });
    }

    for zg in &run.zip_groups {
        axes.push(Axis::Zip {
            members: zg.members.clone(),
            rows: zg.values.clone(),
        });
    }

    let total: u128 = axes.iter().map(|a| a.len() as u128).product();

    (axes, total)
}

pub struct CartesianIter {
    axes: Vec<Axis>,
    strides: Vec<u128>,
    current: u128,
    total: u128,
}

impl CartesianIter {
    pub fn new(axes: Vec<Axis>, total: u128) -> Self {
        let n = axes.len();
        let mut strides = vec![1u128; n];
        for i in (0..n.saturating_sub(1)).rev() {
            strides[i] = strides[i + 1] * axes[i + 1].len() as u128;
        }
        Self {
            axes,
            strides,
            current: 0,
            total,
        }
    }

    pub fn total(&self) -> u128 {
        self.total
    }

    pub fn combo_at(&self, flat_index: u128) -> ParamCombo {
        let mut combo = ParamCombo::new();
        let mut remainder = flat_index;
        for (i, axis) in self.axes.iter().enumerate() {
            let axis_index = (remainder / self.strides[i]) as usize;
            remainder %= self.strides[i];
            axis.write_to(axis_index, &mut combo);
        }
        combo
    }
}

impl Iterator for CartesianIter {
    type Item = ParamCombo;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.total {
            return None;
        }
        let combo = self.combo_at(self.current);
        self.current += 1;
        Some(combo)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.total - self.current).min(usize::MAX as u128) as usize;
        (remaining, Some(remaining))
    }
}

pub struct CartesianRange {
    axes: Vec<Axis>,
    strides: Vec<u128>,
    start: u128,
    end: u128,
}

impl CartesianRange {
    pub fn new(axes: Vec<Axis>, total: u128, start: u128, end: u128) -> Self {
        let n = axes.len();
        let mut strides = vec![1u128; n];
        for i in (0..n.saturating_sub(1)).rev() {
            strides[i] = strides[i + 1] * axes[i + 1].len() as u128;
        }
        Self {
            axes,
            strides,
            start,
            end: end.min(total),
        }
    }

    pub fn len(&self) -> u128 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn combo_at(&self, flat_index: u128) -> ParamCombo {
        let mut combo = ParamCombo::new();
        let mut remainder = flat_index;
        for (i, axis) in self.axes.iter().enumerate() {
            let axis_index = (remainder / self.strides[i]) as usize;
            remainder %= self.strides[i];
            axis.write_to(axis_index, &mut combo);
        }
        combo
    }

    pub fn iter(&self) -> impl Iterator<Item = ParamCombo> + '_ {
        (self.start..self.end).map(|idx| self.combo_at(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::ZipGroup;
    use serde_json::json;

    fn make_run(axes: BTreeMap<String, Vec<Value>>) -> RunTemplate {
        RunTemplate {
            name: "test".into(),
            hash_mode: crate::blueprint::HashMode::ParamsOnly,
            inter_run_dep_types: BTreeMap::new(),
            parameter_axes: axes,
            zip_groups: vec![],
            pipelines: vec![],
            image_path: None,
            image_contents: vec![],
        }
    }

    #[test]
    fn test_single_axis() {
        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1), json!(2), json!(3)]);
        let run = make_run(axes);
        let (ax, total) = build_axes(&run);
        assert_eq!(total, 3);
        let iter = CartesianIter::new(ax, total);
        let combos: Vec<_> = iter.collect();
        assert_eq!(combos.len(), 3);
        assert_eq!(combos[0]["x"], json!(1));
        assert_eq!(combos[2]["x"], json!(3));
    }

    #[test]
    fn test_two_axes() {
        let mut axes = BTreeMap::new();
        axes.insert("a".into(), vec![json!(1), json!(2)]);
        axes.insert("b".into(), vec![json!("x"), json!("y"), json!("z")]);
        let run = make_run(axes);
        let (ax, total) = build_axes(&run);
        assert_eq!(total, 6);
        let iter = CartesianIter::new(ax, total);
        let combos: Vec<_> = iter.collect();
        assert_eq!(combos.len(), 6);
    }

    #[test]
    fn test_zip_group() {
        let mut axes = BTreeMap::new();
        axes.insert("a".into(), vec![json!(1), json!(2)]);
        axes.insert("m".into(), vec![json!(10), json!(20)]);
        axes.insert("s".into(), vec![json!(100), json!(200)]);

        let mut run = make_run(axes);
        run.zip_groups = vec![ZipGroup {
            members: vec!["m".into(), "s".into()],
            values: vec![
                {
                    let mut r = BTreeMap::new();
                    r.insert("m".into(), json!(10));
                    r.insert("s".into(), json!(100));
                    r
                },
                {
                    let mut r = BTreeMap::new();
                    r.insert("m".into(), json!(20));
                    r.insert("s".into(), json!(200));
                    r
                },
            ],
        }];

        let (ax, total) = build_axes(&run);
        assert_eq!(total, 4);
        let iter = CartesianIter::new(ax, total);
        let combos: Vec<_> = iter.collect();
        assert_eq!(combos.len(), 4);
        for combo in &combos {
            if combo["m"] == json!(10) {
                assert_eq!(combo["s"], json!(100));
            } else {
                assert_eq!(combo["s"], json!(200));
            }
        }
    }

    #[test]
    fn test_range_matches_full() {
        let mut axes_map = BTreeMap::new();
        axes_map.insert("a".into(), vec![json!(1), json!(2)]);
        axes_map.insert("b".into(), vec![json!("x"), json!("y")]);
        let run = make_run(axes_map);
        let (ax, total) = build_axes(&run);

        let full: Vec<_> = CartesianIter::new(
            {
                let (ax2, _) = build_axes(&run);
                ax2
            },
            total,
        )
        .collect();

        let range = CartesianRange::new(ax, total, 0, total);
        let ranged: Vec<_> = range.iter().collect();

        assert_eq!(full, ranged);
    }
}
