//! Stable branch lanes and the centerline of the supplied 35% logo curve.
use crate::history::Revision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphHalf {
    Incoming,
    Outgoing,
}
#[derive(Clone, Debug)]
pub struct GraphStroke {
    pub from: usize,
    pub to: usize,
    pub color: usize,
    pub half: GraphHalf,
    layer: usize,
}
impl GraphStroke {
    pub fn layer(&self) -> usize {
        self.layer
    }
}
#[derive(Clone, Debug)]
pub struct GraphRow {
    pub lane: usize,
    pub color: usize,
    /// Back-to-front paint order. Rightmost branches remain above inner lanes.
    pub strokes: Vec<GraphStroke>,
}
struct Branch<'a> {
    target: &'a str,
    color: usize,
    origin: usize,
}

pub(crate) fn layout(revisions: &[Revision]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<Branch<'_>>> = Vec::new();
    let mut next_color = 0;
    let mut rows: Vec<GraphRow> = Vec::with_capacity(revisions.len());
    for revision in revisions {
        let lane = lanes
            .iter()
            .position(|branch| {
                branch
                    .as_ref()
                    .is_some_and(|b| b.target == revision.commit_id)
            })
            .unwrap_or_else(|| {
                lanes
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(lanes.len())
            });
        let color = if let Some(Some(branch)) = lanes.get(lane) {
            branch.color
        } else {
            let color = next_color;
            next_color += 1;
            color
        };
        let mut strokes = Vec::new();
        // Equal targets keep separate lanes until their actual commit. Each
        // center-to-center link is split between the two independently painted
        // rows, preserving the same curve/color/layer on both sides of a stripe.
        for (slot, branch) in lanes.iter_mut().enumerate() {
            if let Some(active) = branch.as_mut() {
                let joins = active.target == revision.commit_id;
                let to = if joins { lane } else { slot };
                let incoming = GraphStroke {
                    from: active.origin,
                    to,
                    color: active.color,
                    half: GraphHalf::Incoming,
                    layer: slot,
                };
                if let Some(previous) = rows.last_mut() {
                    previous.strokes.push(GraphStroke {
                        half: GraphHalf::Outgoing,
                        ..incoming.clone()
                    });
                }
                strokes.push(incoming);
                active.origin = slot;
                if joins {
                    *branch = None;
                }
            }
        }
        if let Some(previous) = rows.last_mut() {
            previous.strokes.sort_by_key(GraphStroke::layer);
        }
        if lane == lanes.len() {
            lanes.push(None);
        }
        for (index, parent) in revision.parents.iter().enumerate() {
            let (to, parent_color) = if index == 0 {
                (lane, color)
            } else {
                let to = lanes
                    .iter()
                    .enumerate()
                    .skip(lane + 1)
                    .find_map(|(slot, branch)| branch.is_none().then_some(slot))
                    .unwrap_or(lanes.len());
                if to == lanes.len() {
                    lanes.push(None);
                }
                let color = next_color;
                next_color += 1;
                (to, color)
            };
            lanes[to] = Some(Branch {
                target: parent,
                color: parent_color,
                origin: lane,
            });
        }
        strokes.sort_by_key(GraphStroke::layer);
        rows.push(GraphRow {
            lane,
            color,
            strokes,
        });
        // Reuse free slots for new branches, but never compact occupied lanes.
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
    }
    // Parents outside the query limit continue into the clipped bottom edge.
    if let Some(last) = rows.last_mut() {
        for (slot, branch) in lanes.iter().enumerate() {
            if let Some(branch) = branch {
                last.strokes.push(GraphStroke {
                    from: branch.origin,
                    to: slot,
                    color: branch.color,
                    half: GraphHalf::Outgoing,
                    layer: slot,
                });
            }
        }
        last.strokes.sort_by_key(GraphStroke::layer);
    }
    rows
}

#[derive(Clone, Copy, Debug)]
pub struct Cubic {
    pub start: [f32; 2],
    pub control1: [f32; 2],
    pub control2: [f32; 2],
    pub end: [f32; 2],
}
/// Midpoints of paired outline Béziers in assets/branch-curve-35pct.svg.
/// The four middle segments retain the supplied geometry. The end segments
/// extend inside the logo's circular markers to their centers (80,432) and
/// (368,80), with vertical endpoint tangents for seamless row boundaries.
const CENTERLINE: [[[f32; 2]; 4]; 6] = [
    [[80., 432.], [80., 408.], [83.325, 384.08], [92.07, 359.92]],
    [
        [92.07, 359.92],
        [100.815, 335.76],
        [120.045, 311.115],
        [143.96, 294.03],
    ],
    [
        [143.96, 294.03],
        [167.875, 276.945],
        [196.605, 266.79],
        [224., 256.],
    ],
    [
        [224., 256.],
        [251.395, 245.21],
        [280.125, 235.055],
        [304.04, 217.97],
    ],
    [
        [304.04, 217.97],
        [327.955, 200.885],
        [347.185, 176.24],
        [355.93, 152.08],
    ],
    [
        [355.93, 152.08],
        [364.675, 127.92],
        [368., 104.],
        [368., 80.],
    ],
];
/// Map the logo's centerline to an arbitrary pair of screen-space endpoints.
/// A negative horizontal span mirrors it for branches returning leftward.
pub fn branch_curve(start: [f32; 2], end: [f32; 2]) -> [Cubic; 6] {
    let map = |p: [f32; 2]| {
        [
            start[0] + (p[0] - 80.) / 288. * (end[0] - start[0]),
            start[1] + (432. - p[1]) / 352. * (end[1] - start[1]),
        ]
    };
    CENTERLINE.map(|[a, b, c, d]| Cubic {
        start: map(a),
        control1: map(b),
        control2: map(c),
        end: map(d),
    })
}

#[cfg(test)]
mod tests {
    use super::{GraphHalf, branch_curve};
    use crate::history::History;
    #[test]
    fn nested_branches_stay_in_their_lanes_and_fold_into_the_shared_dot() {
        let history = History::parse(concat!(
            "[\"a\",\"a\",\"merge\",\"Test\",[],[\"b\",\"c\"]]\n",
            "[\"b\",\"b\",\"nested merge\",\"Test\",[],[\"d\",\"e\"]]\n",
            "[\"c\",\"c\",\"outer\",\"Test\",[],[\"f\"]]\n",
            "[\"d\",\"d\",\"main\",\"Test\",[],[\"f\"]]\n",
            "[\"e\",\"e\",\"rightmost\",\"Test\",[],[\"f\"]]\n",
            "[\"f\",\"f\",\"join\",\"Test\",[],[]]\n"
        ))
        .unwrap();
        assert_eq!(
            history.graph.iter().map(|row| row.lane).collect::<Vec<_>>(),
            [0, 0, 1, 0, 2, 0]
        );
        assert_eq!(
            history
                .graph
                .iter()
                .map(|row| row.color)
                .collect::<Vec<_>>(),
            [0, 0, 1, 0, 2, 0]
        );
        let join = &history.graph[5];
        assert_eq!(
            join.strokes
                .iter()
                .map(|s| (s.from, s.to, s.color, s.half))
                .collect::<Vec<_>>(),
            [
                (0, 0, 0, GraphHalf::Incoming),
                (1, 0, 1, GraphHalf::Incoming),
                (2, 0, 2, GraphHalf::Incoming)
            ]
        );
        for pair in history.graph.windows(2) {
            let outgoing = pair[0]
                .strokes
                .iter()
                .filter(|s| s.half == GraphHalf::Outgoing)
                .map(|s| (s.from, s.to, s.color, s.layer()))
                .collect::<Vec<_>>();
            let incoming = pair[1]
                .strokes
                .iter()
                .filter(|s| s.half == GraphHalf::Incoming)
                .map(|s| (s.from, s.to, s.color, s.layer()))
                .collect::<Vec<_>>();
            assert_eq!(
                outgoing, incoming,
                "adjacent rows must paint the same route"
            );
        }
        for row in &history.graph {
            assert!(
                row.strokes
                    .windows(2)
                    .all(|pair| pair[0].layer() <= pair[1].layer())
            );
        }
        assert_eq!(history.visible_lane_count(5..6), 3);
    }
    #[test]
    fn forks_reuse_inner_holes_without_shifting_an_active_outer_branch() {
        let history = History::parse(concat!(
            "[\"a\",\"a\",\"merge\",\"Test\",[],[\"b\",\"c\",\"d\"]]\n",
            "[\"c\",\"c\",\"joins main\",\"Test\",[],[\"b\"]]\n",
            "[\"b\",\"b\",\"new fork\",\"Test\",[],[\"e\",\"f\"]]\n",
            "[\"e\",\"e\",\"main\",\"Test\",[],[\"ab\"]]\n",
            "[\"f\",\"f\",\"new inner branch\",\"Test\",[],[\"ab\"]]\n",
            "[\"d\",\"d\",\"existing outer branch\",\"Test\",[],[\"ab\"]]\n",
            "[\"ab\",\"ab\",\"baseline\",\"Test\",[],[]]\n"
        ))
        .unwrap();
        assert_eq!(
            history.graph[4].lane, 1,
            "the completed inner branch frees lane one"
        );
        assert_eq!(
            (history.graph[5].lane, history.graph[5].color),
            (2, 2),
            "the outer branch must not move or change color"
        );
        assert_eq!(history.lane_count, 3);
    }
    #[test]
    fn independent_head_reuses_a_free_lane() {
        let history = History::parse(concat!(
            "[\"a\",\"a\",\"merge\",\"Test\",[],[\"b\",\"c\",\"d\"]]\n",
            "[\"c\",\"c\",\"joins main\",\"Test\",[],[\"b\"]]\n",
            "[\"b\",\"b\",\"main\",\"Test\",[],[\"e\"]]\n",
            "[\"f\",\"f\",\"independent head\",\"Test\",[],[\"ab\"]]\n",
            "[\"e\",\"e\",\"main\",\"Test\",[],[\"ab\"]]\n",
            "[\"d\",\"d\",\"outer\",\"Test\",[],[\"ab\"]]\n",
            "[\"ab\",\"ab\",\"baseline\",\"Test\",[],[]]\n"
        ))
        .unwrap();
        assert_eq!(history.graph[3].lane, 1);
        assert_eq!(history.graph[5].lane, 2);
    }
    #[test]
    fn supplied_curve_is_continuous_mirrored_and_has_vertical_end_tangents() {
        for (start, end) in [([0., 0.], [12., 13.]), ([24., 13.], [0., 26.])] {
            let curves = branch_curve(start, end);
            assert_eq!(curves[0].start, start);
            assert_eq!(curves[5].end, end);
            assert_eq!(curves[0].control1[0], start[0]);
            assert_eq!(curves[5].control2[0], end[0]);
            for pair in curves.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
                for axis in 0..2 {
                    let incoming = pair[0].end[axis] - pair[0].control2[axis];
                    let outgoing = pair[1].control1[axis] - pair[1].start[axis];
                    assert!((incoming - outgoing).abs() < 0.001);
                }
            }
            assert_eq!(
                curves[2].end,
                [(start[0] + end[0]) / 2., (start[1] + end[1]) / 2.]
            );
        }
    }
}
