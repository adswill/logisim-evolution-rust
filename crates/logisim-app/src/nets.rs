//! Net analysis for the editor: which wires form one net, and a stable, well-separated hue for
//! each net. Pure model code (no UI); the canvas only maps hue indexes to theme colours.
//!
//! Connectivity mirrors the simulator: wires join at shared endpoints and ports, an endpoint or
//! port lying on a wire joins that wire (T-junction), tunnels of equal name join, and wires that
//! merely cross do not.

use crate::model::P;
use logisim_format::{CircuitDef, ComponentDef};
use std::collections::{HashMap, HashSet};

/// Number of distinct hues (a colourblind-friendly palette, see `paint::Theme::net_color`).
pub const HUES: usize = 10;
/// Nets whose wires come this close (world units, 2 grid steps) count as neighbours.
pub const NEIGHBOUR_DISTANCE: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    /// Indexes into `CircuitDef::wires`.
    pub wires: Vec<usize>,
    /// Every wire end and port point of the net.
    pub points: Vec<P>,
    /// Hue index in `0..HUES`.
    pub hue: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetAnalysis {
    pub nets: Vec<Net>,
    /// For each wire, the index of its net.
    pub wire_net: Vec<usize>,
}

/// Remembers the previous nets so hues stay put when the circuit is edited.
#[derive(Debug, Clone, Default)]
pub struct HueMemory {
    prev: Vec<(HashSet<P>, usize)>,
}

struct Uf(Vec<usize>);
impl Uf {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}

fn on_segment(p: P, a: P, b: P) -> bool {
    p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

fn tunnel_name(d: &ComponentDef) -> Option<&str> {
    (d.library == "Wiring" && d.part == "Tunnel")
        .then(|| d.options.string("name").ok().flatten())
        .flatten()
}

/// Distance between two axis-aligned segments (0 when they touch or cross).
fn segment_distance(a: (P, P), b: (P, P)) -> f32 {
    let r = |s: (P, P)| {
        (
            s.0 .0.min(s.1 .0),
            s.0 .1.min(s.1 .1),
            s.0 .0.max(s.1 .0),
            s.0 .1.max(s.1 .1),
        )
    };
    let (ra, rb) = (r(a), r(b));
    let dx = (ra.0 - rb.2).max(rb.0 - ra.2).max(0) as f32;
    let dy = (ra.1 - rb.3).max(rb.1 - ra.3).max(0) as f32;
    dx.hypot(dy)
}

impl HueMemory {
    /// Group the wires of `c` into nets and give each a hue. `ports[i]` are the absolute port
    /// points of component `i`. Hues of nets that survive an edit are kept where possible.
    pub fn analyze(&mut self, c: &CircuitDef, ports: &[Vec<P>]) -> NetAnalysis {
        // ---- connectivity
        let mut pts: Vec<P> = c
            .wires
            .iter()
            .flat_map(|w| [w.a, w.b])
            .chain(ports.iter().flatten().copied())
            .collect();
        pts.sort_unstable();
        pts.dedup();
        let idx: HashMap<P, usize> = pts.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let mut uf = Uf((0..pts.len()).collect());
        for w in &c.wires {
            let first = idx[&w.a];
            for (p, &i) in &idx {
                if on_segment(*p, w.a, w.b) {
                    uf.union(first, i);
                }
            }
        }
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        for (d, ps) in c.components.iter().zip(ports) {
            if let (Some(name), Some(p)) = (tunnel_name(d), ps.first()) {
                let i = idx[p];
                match by_name.get(name) {
                    Some(&j) => uf.union(i, j),
                    None => {
                        by_name.insert(name, i);
                    }
                }
            }
        }
        // ---- nets (only those with at least one wire are drawn)
        let mut net_of_root: HashMap<usize, usize> = HashMap::new();
        let mut nets: Vec<Net> = vec![];
        let mut wire_net = vec![0; c.wires.len()];
        for (wi, w) in c.wires.iter().enumerate() {
            let root = uf.find(idx[&w.a]);
            let n = *net_of_root.entry(root).or_insert_with(|| {
                nets.push(Net {
                    wires: vec![],
                    points: vec![],
                    hue: 0,
                });
                nets.len() - 1
            });
            nets[n].wires.push(wi);
            wire_net[wi] = n;
        }
        for (i, p) in pts.iter().enumerate() {
            let root = uf.find(i);
            if let Some(&n) = net_of_root.get(&root) {
                nets[n].points.push(*p);
            }
        }
        // ---- neighbours: nearby wires, and nets meeting at the same part
        let n = nets.len();
        let mut adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
        for i in 0..n {
            for j in i + 1..n {
                let near = nets[i].wires.iter().any(|&a| {
                    nets[j].wires.iter().any(|&b| {
                        segment_distance((c.wires[a].a, c.wires[a].b), (c.wires[b].a, c.wires[b].b))
                            <= NEIGHBOUR_DISTANCE
                    })
                });
                if near {
                    adj[i].insert(j);
                    adj[j].insert(i);
                }
            }
        }
        for ps in ports {
            let touching: Vec<usize> = ps
                .iter()
                .filter_map(|p| {
                    idx.get(p)
                        .and_then(|&i| net_of_root.get(&uf.find(i)).copied())
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            for &a in &touching {
                for &b in &touching {
                    if a != b {
                        adj[a].insert(b);
                    }
                }
            }
        }
        // ---- inherit hues from the previous analysis (best point overlap, one-to-one)
        let sets: Vec<HashSet<P>> = nets
            .iter()
            .map(|net| net.points.iter().copied().collect())
            .collect();
        let mut pairs: Vec<(usize, usize, usize)> = vec![]; // (overlap, new, prev)
        for (i, s) in sets.iter().enumerate() {
            for (j, (ps, _)) in self.prev.iter().enumerate() {
                let o = s.intersection(ps).count();
                if o > 0 {
                    pairs.push((o, i, j));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut hue: Vec<Option<usize>> = vec![None; n];
        let mut prev_taken = vec![false; self.prev.len()];
        for (_, i, j) in pairs {
            if hue[i].is_some() || prev_taken[j] {
                continue;
            }
            prev_taken[j] = true;
            let h = self.prev[j].1;
            if adj[i].iter().all(|&nb| hue[nb] != Some(h)) {
                hue[i] = Some(h);
            }
        }
        // ---- everything else: greedy, most-constrained first, avoiding neighbours' hues
        let mut used_total = [0usize; HUES];
        for h in hue.iter().flatten() {
            used_total[*h] += 1;
        }
        let mut order: Vec<usize> = (0..n).filter(|&i| hue[i].is_none()).collect();
        order.sort_by(|&a, &b| adj[b].len().cmp(&adj[a].len()).then(a.cmp(&b)));
        for i in order {
            let mut near = [0usize; HUES];
            for &nb in &adj[i] {
                if let Some(h) = hue[nb] {
                    near[h] += 1;
                }
            }
            // fewest conflicting neighbours first, then the globally least used hue, then lowest index
            let best = (0..HUES)
                .min_by_key(|&h| (near[h], used_total[h], h))
                .unwrap_or(0);
            hue[i] = Some(best);
            used_total[best] += 1;
        }
        for (i, net) in nets.iter_mut().enumerate() {
            net.hue = hue[i].unwrap_or(0);
        }
        self.prev = sets.into_iter().zip(nets.iter().map(|n| n.hue)).collect();
        NetAnalysis { nets, wire_net }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logisim_format::WireDef;

    fn circuit(wires: &[(P, P)]) -> CircuitDef {
        CircuitDef {
            name: "t".into(),
            interface: vec![],
            components: vec![],
            wires: wires
                .iter()
                .map(|(a, b)| WireDef { a: *a, b: *b })
                .collect(),
        }
    }

    fn analyze(mem: &mut HueMemory, c: &CircuitDef) -> NetAnalysis {
        let ports: Vec<Vec<P>> = vec![vec![]; c.components.len()];
        mem.analyze(c, &ports)
    }

    #[test]
    fn connectivity_endpoints_tjunctions_and_crossings() {
        let c = circuit(&[
            ((0, 0), (100, 0)),    // 0: horizontal
            ((50, 0), (50, 60)),   // 1: ends on wire 0 (T-junction) → same net
            ((100, 0), (100, 40)), // 2: shares an endpoint with wire 0 → same net
            ((0, 30), (90, 30)), // 3: crosses wire 1 at (50,30) without an endpoint → separate net
        ]);
        let a = analyze(&mut HueMemory::default(), &c);
        assert_eq!(a.nets.len(), 2);
        assert_eq!(a.wire_net[0], a.wire_net[1]);
        assert_eq!(a.wire_net[0], a.wire_net[2]);
        assert_ne!(a.wire_net[0], a.wire_net[3]);
    }

    #[test]
    fn all_wires_of_a_net_share_one_hue_and_neighbours_differ() {
        // three parallel nets 10 apart (neighbours), each made of two wires
        let c = circuit(&[
            ((0, 0), (50, 0)),
            ((50, 0), (100, 0)),
            ((0, 10), (50, 10)),
            ((50, 10), (100, 10)),
            ((0, 20), (50, 20)),
            ((50, 20), (100, 20)),
            ((500, 500), (550, 500)), // far away
        ]);
        let a = analyze(&mut HueMemory::default(), &c);
        assert_eq!(a.nets.len(), 4);
        let h = |w: usize| a.nets[a.wire_net[w]].hue;
        assert_eq!(h(0), h(1));
        assert_eq!(h(2), h(3));
        assert_ne!(h(0), h(2));
        assert_ne!(h(2), h(4));
        assert_ne!(h(0), h(4));
    }

    #[test]
    fn nets_on_the_same_part_get_different_hues() {
        // two gate-ish ports far apart in distance but on one component
        let mut c = circuit(&[((0, 0), (40, 0)), ((0, 200), (40, 200))]);
        c.components
            .push(ComponentDef::new("Gates", "AND Gate", 100, 100).unwrap());
        let ports = vec![vec![(40, 0), (40, 200)]];
        let a = HueMemory::default().analyze(&c, &ports);
        assert_eq!(a.nets.len(), 2);
        assert_ne!(a.nets[0].hue, a.nets[1].hue);
    }

    #[test]
    fn tunnels_join_nets_with_the_same_name() {
        let mut c = circuit(&[((0, 0), (20, 0)), ((300, 300), (320, 300))]);
        for (x, y) in [(20, 0), (300, 300)] {
            c.components.push(
                ComponentDef::new("Wiring", "Tunnel", x, y)
                    .unwrap()
                    .option("name", "bus"),
            );
        }
        let ports = vec![vec![(20, 0)], vec![(300, 300)]];
        let a = HueMemory::default().analyze(&c, &ports);
        assert_eq!(a.nets.len(), 1);
        assert_eq!(a.nets[0].wires.len(), 2);
    }

    #[test]
    fn hues_are_stable_after_unrelated_edits() {
        let mut c = circuit(&[
            ((0, 0), (100, 0)),
            ((0, 10), (100, 10)),
            ((0, 20), (100, 20)),
            ((400, 0), (500, 0)),
        ]);
        let mut mem = HueMemory::default();
        let before = analyze(&mut mem, &c);
        let hues = |a: &NetAnalysis, c: &CircuitDef| -> Vec<usize> {
            (0..c.wires.len())
                .map(|w| a.nets[a.wire_net[w]].hue)
                .collect()
        };
        let h0 = hues(&before, &c);
        // unrelated edit 1: add a wire far away
        c.wires.push(WireDef {
            a: (900, 900),
            b: (950, 900),
        });
        let a1 = analyze(&mut mem, &c);
        assert_eq!(&hues(&a1, &c)[..4], &h0[..]);
        // unrelated edit 2: remove the far wire and insert one at the start of the list
        c.wires.pop();
        c.wires.insert(
            0,
            WireDef {
                a: (900, 900),
                b: (950, 900),
            },
        );
        let a2 = analyze(&mut mem, &c);
        assert_eq!(&hues(&a2, &c)[1..], &h0[..]);
        // unrelated edit 3: extend a net (a new segment meeting its end keeps its hue)
        c.wires.push(WireDef {
            a: (100, 0),
            b: (100, -40),
        });
        let a3 = analyze(&mut mem, &c);
        assert_eq!(hues(&a3, &c)[1], h0[0]);
        assert_eq!(
            a3.nets[a3.wire_net[1]].hue,
            a3.nets[a3.wire_net[c.wires.len() - 1]].hue
        );
    }

    #[test]
    fn a_new_neighbour_never_steals_a_hue() {
        let mut c = circuit(&[((0, 0), (100, 0))]);
        let mut mem = HueMemory::default();
        let first = analyze(&mut mem, &c).nets[0].hue;
        c.wires.push(WireDef {
            a: (0, 10),
            b: (100, 10),
        });
        let a = analyze(&mut mem, &c);
        assert_eq!(a.nets[a.wire_net[0]].hue, first);
        assert_ne!(a.nets[a.wire_net[0]].hue, a.nets[a.wire_net[1]].hue);
    }

    #[test]
    fn more_neighbours_than_hues_still_colours_everything() {
        let wires: Vec<(P, P)> = (0..HUES as i32 + 4)
            .map(|i| ((0, i * 2), (100, i * 2)))
            .collect();
        let a = analyze(&mut HueMemory::default(), &circuit(&wires));
        assert_eq!(a.nets.len(), HUES + 4);
        assert!(a.nets.iter().all(|n| n.hue < HUES));
        // all hues are used before any is reused
        let used: HashSet<usize> = a.nets.iter().map(|n| n.hue).collect();
        assert_eq!(used.len(), HUES);
    }

    #[test]
    fn splitting_a_net_keeps_the_hue_on_the_larger_half() {
        let mut c = circuit(&[
            ((0, 0), (200, 0)),
            ((200, 0), (400, 0)),
            ((400, 0), (450, 0)),
        ]);
        let mut mem = HueMemory::default();
        let a = analyze(&mut mem, &c);
        assert_eq!(a.nets.len(), 1);
        let h = a.nets[0].hue;
        c.wires.remove(1); // wire 0 and wire 1(old 2) are now separate nets
        let b = analyze(&mut mem, &c);
        assert_eq!(b.nets.len(), 2);
        assert_eq!(b.nets[b.wire_net[0]].hue, h); // 200-long half keeps it
        assert_ne!(b.nets[0].hue, b.nets[1].hue);
    }
}
