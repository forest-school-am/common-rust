//! The access-control algebra: predicates over a `Principal` and the state,
//! composed with `And`/`Or`/`Not`, and the `Denial` a failed check carries.
//! Turning a `Denial` into a response is auth.rs; resolving identity is not
//! done here at all.

use std::marker::PhantomData;

use crate::principal::Principal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub gate: String,
}

impl Denial {
    pub fn new(gate: impl Into<String>) -> Self {
        Self { gate: gate.into() }
    }

    pub fn group(group: Option<String>) -> Self {
        Self {
            gate: match group {
                Some(g) => format!("{g} (effective membership)"),
                None => "-".to_owned(),
            },
        }
    }
}

pub trait Predicate<S>: 'static {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial>;
}

pub trait Group<S>: 'static {
    fn group(state: &S) -> Option<String>;
}

/// An unconfigured group (`None`) never passes.
pub struct HasGroup<G>(PhantomData<G>);

impl<S, G: Group<S>> Predicate<S> for HasGroup<G> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        let group = G::group(state);
        match &group {
            Some(g) if principal.in_group(g) => Ok(()),
            _ => Err(Denial::group(group)),
        }
    }
}

pub struct Or<A, B>(PhantomData<(A, B)>);

impl<S, A: Predicate<S>, B: Predicate<S>> Predicate<S> for Or<A, B> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        match A::check(principal, state) {
            Ok(()) => Ok(()),
            Err(a) => match B::check(principal, state) {
                Ok(()) => Ok(()),
                Err(b) => Err(Denial {
                    gate: format!("{} or {}", a.gate, b.gate),
                }),
            },
        }
    }
}

pub struct And<A, B>(PhantomData<(A, B)>);

impl<S, A: Predicate<S>, B: Predicate<S>> Predicate<S> for And<A, B> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        A::check(principal, state)?;
        B::check(principal, state)
    }
}

pub struct Not<A>(PhantomData<A>);

impl<S, A: Predicate<S>> Predicate<S> for Not<A> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        match A::check(principal, state) {
            Ok(()) => Err(Denial {
                gate: "must not satisfy the excluded rule".to_owned(),
            }),
            Err(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GATE: &str = "gate-group";
    const INDEX: &str = "index-group";

    struct TestState {
        gate: Option<String>,
        index: Option<String>,
    }

    struct GateGroup;
    struct IndexGroup;
    impl Group<TestState> for GateGroup {
        fn group(s: &TestState) -> Option<String> {
            s.gate.clone()
        }
    }
    impl Group<TestState> for IndexGroup {
        fn group(s: &TestState) -> Option<String> {
            s.index.clone()
        }
    }

    fn state() -> TestState {
        TestState {
            gate: Some(GATE.into()),
            index: Some(INDEX.into()),
        }
    }

    fn principal(groups: &[&str]) -> Principal {
        Principal {
            username: "t".into(),
            effective_groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        }
    }

    fn check<P: Predicate<TestState>>(groups: &[&str], state: &TestState) -> bool {
        P::check(&principal(groups), state).is_ok()
    }

    #[test]
    fn combinator_truth_table() {
        let state = state();
        type G = HasGroup<GateGroup>;
        type I = HasGroup<IndexGroup>;

        assert!(check::<Or<G, I>>(&[GATE], &state));
        assert!(check::<Or<G, I>>(&[INDEX], &state));
        assert!(check::<Or<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<Or<G, I>>(&["random-group"], &state));

        assert!(check::<And<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<And<G, I>>(&[GATE], &state));
        assert!(!check::<And<G, I>>(&[INDEX], &state));

        assert!(check::<Not<G>>(&[INDEX], &state));
        assert!(!check::<Not<G>>(&[GATE], &state));
    }

    #[test]
    fn an_unconfigured_group_never_passes() {
        let state = TestState {
            gate: None,
            index: None,
        };
        assert!(!check::<HasGroup<GateGroup>>(&[GATE], &state));
    }

    #[test]
    fn or_denial_names_both_required_groups() {
        let denial =
            <Or<HasGroup<GateGroup>, HasGroup<IndexGroup>>>::check(&principal(&[]), &state())
                .expect_err("no group -> denied");
        assert!(denial.gate.contains(GATE), "{}", denial.gate);
        assert!(denial.gate.contains(INDEX), "{}", denial.gate);
        assert!(denial.gate.contains(" or "), "{}", denial.gate);
    }
}
