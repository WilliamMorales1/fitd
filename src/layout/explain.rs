
pub(super) fn quickxplain(
    feasible: &mut dyn FnMut(&[usize]) -> bool,
    base: &[usize],
    added: bool,
    cs: &[usize],
) -> Vec<usize> {
    if added && !feasible(base) {
        return Vec::new();
    }
    if cs.len() == 1 {
        return cs.to_vec();
    }
    let (c1, c2) = cs.split_at(cs.len() / 2);
    let d2 = quickxplain(feasible, &[base, c1].concat(), !c1.is_empty(), c2);
    let d1 = quickxplain(feasible, &[base, &d2].concat(), !d2.is_empty(), c1);
    [d1, d2].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quickxplain_finds_minimal_core() {
        let mut calls = 0;
        let mut feasible = |s: &[usize]| {
            calls += 1;
            !(s.contains(&2) && s.contains(&5)) && !s.contains(&7)
        };
        let mut core = quickxplain(&mut feasible, &[], false, &[0, 1, 2, 3, 4, 5, 6]);
        core.sort();
        assert_eq!(core, vec![2, 5]);
        assert!(calls < 20, "{calls} oracle calls");
    }
}
