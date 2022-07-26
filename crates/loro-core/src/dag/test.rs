#![cfg(test)]

use proptest::proptest;

use super::*;
use crate::{
    change::Lamport,
    id::{ClientID, Counter, ID},
    span::{CounterSpan, IdSpan},
};
use std::collections::HashSet;
use std::iter::FromIterator;

#[derive(Debug, PartialEq, Eq, Clone)]
struct TestNode {
    id: ID,
    lamport: Lamport,
    len: usize,
    deps: Vec<ID>,
}

impl TestNode {
    fn new(id: ID, lamport: Lamport, deps: Vec<ID>, len: usize) -> Self {
        Self {
            id,
            lamport,
            deps,
            len,
        }
    }
}

impl DagNode for TestNode {
    fn dag_id_start(&self) -> ID {
        self.id
    }
    fn lamport_start(&self) -> Lamport {
        self.lamport
    }
    fn len(&self) -> usize {
        self.len
    }
    fn deps(&self) -> &Vec<ID> {
        &self.deps
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TestDag {
    nodes: FxHashMap<ClientID, Vec<TestNode>>,
    frontier: Vec<ID>,
    version_vec: FxHashMap<ClientID, Counter>,
    next_lamport: Lamport,
    client_id: ClientID,
}

impl Dag for TestDag {
    type Node = TestNode;

    fn get(&self, id: ID) -> Option<&Self::Node> {
        self.nodes.get(&id.client_id)?.iter().find(|node| {
            id.counter >= node.id.counter && id.counter < node.id.counter + node.len as Counter
        })
    }
    fn frontier(&self) -> &[ID] {
        &self.frontier
    }

    fn roots(&self) -> Vec<&Self::Node> {
        self.nodes.iter().map(|(_, v)| &v[0]).collect()
    }

    fn contains(&self, id: ID) -> bool {
        self.version_vec
            .get(&id.client_id)
            .and_then(|x| if *x > id.counter { Some(()) } else { None })
            .is_some()
    }
}

impl TestDag {
    pub fn new(client_id: ClientID) -> Self {
        Self {
            nodes: FxHashMap::default(),
            frontier: Vec::new(),
            version_vec: FxHashMap::default(),
            next_lamport: 0,
            client_id,
        }
    }

    fn push(&mut self, len: usize) {
        let client_id = self.client_id;
        let counter = self.version_vec.entry(client_id).or_insert(0);
        let id = ID::new(client_id, *counter);
        *counter += len as u32;
        let deps = std::mem::replace(&mut self.frontier, vec![id]);
        self.nodes
            .entry(client_id)
            .or_insert(vec![])
            .push(TestNode::new(id, self.next_lamport, deps, len));
        self.next_lamport += len as u32;
    }

    fn merge(&mut self, other: &TestDag) {
        let mut pending = Vec::new();
        for (_, nodes) in other.nodes.iter() {
            for (i, node) in nodes.iter().enumerate() {
                if self._try_push_node(node, &mut pending, i) {
                    break;
                }
            }
        }

        let mut current = pending;
        let mut pending = Vec::new();
        while !pending.is_empty() || !current.is_empty() {
            if current.is_empty() {
                std::mem::swap(&mut pending, &mut current);
            }

            let (client_id, index) = current.pop().unwrap();
            let node_vec = other.nodes.get(&client_id).unwrap();
            #[allow(clippy::needless_range_loop)]
            for i in index..node_vec.len() {
                let node = &node_vec[i];
                if self._try_push_node(node, &mut pending, i) {
                    break;
                }
            }
        }
    }

    fn _try_push_node(
        &mut self,
        node: &TestNode,
        pending: &mut Vec<(u64, usize)>,
        i: usize,
    ) -> bool {
        let client_id = node.id.client_id;
        if self.contains(node.id) {
            return false;
        }
        if node.deps.iter().any(|dep| !self.contains(*dep)) {
            pending.push((client_id, i));
            return true;
        }
        update_frontier(&mut self.frontier, node.id, &node.deps);
        self.nodes
            .entry(client_id)
            .or_insert(vec![])
            .push(node.clone());
        self.version_vec
            .insert(client_id, node.id.counter + node.len as u32);
        self.next_lamport = self.next_lamport.max(node.lamport + node.len as u32);
        false
    }
}

#[test]
fn test_dag() {
    let mut a = TestDag::new(0);
    let mut b = TestDag::new(1);
    a.push(1);
    assert_eq!(a.frontier().len(), 1);
    assert_eq!(a.frontier()[0].counter, 0);
    b.push(1);
    a.merge(&b);
    assert_eq!(a.frontier().len(), 2);
    a.push(1);
    assert_eq!(a.frontier().len(), 1);
    // a:   0 --(merge)--- 1
    //            ↑
    //            |
    // b:   0 ----
    assert_eq!(
        a.frontier()[0],
        ID {
            client_id: 0,
            counter: 1
        }
    );

    // a:   0 --(merge)--- 1 --- 2 -------
    //            ↑                      |
    //            |                     ↓
    // b:   0 ------------1----------(merge)
    a.push(1);
    b.push(1);
    b.merge(&a);
    assert_eq!(b.next_lamport, 3);
    assert_eq!(b.frontier().len(), 2);
    assert_eq!(
        b.get_common_ancestor(ID::new(0, 2), ID::new(1, 1)),
        Some(ID::new(1, 0))
    );
}

#[cfg(not(no_proptest))]
mod find_common_ancestors {
    use proptest::prelude::*;

    use crate::{array_mut_ref, unsafe_array_mut_ref};

    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct Interaction {
        dag_idx: usize,
        merge_with: Option<usize>,
        len: usize,
    }

    prop_compose! {
        fn gen_interaction(num: usize)(dag_idx in 0..num, merge_with in 0..num, length in 1..10, should_merge in 0..2) -> Interaction {
            Interaction {
                dag_idx,
                merge_with: if should_merge == 1 && merge_with != dag_idx { Some(merge_with) } else { None },
                len: length as usize,
            }
        }
    }

    proptest! {
        #[test]
        fn test_2dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(2), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(2), 0..300)
        ) {
            test(2, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_3dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(3), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(3), 0..300)
        ) {
            test(3, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_4dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(4), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(4), 0..300)
        ) {
            test(4, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_10dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300)
        ) {
            test(10, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_100dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(100), 0..2000),
            after_merged_insertions in prop::collection::vec(gen_interaction(100), 0..2000)
        ) {
            test(100, before_merged_insertions, after_merged_insertions)?;
        }
    }

    fn preprocess(interactions: &mut [Interaction], num: i32) {
        for interaction in interactions.iter_mut() {
            interaction.dag_idx %= num as usize;
            if let Some(ref mut merge_with) = interaction.merge_with {
                *merge_with %= num as usize;
                if *merge_with == interaction.dag_idx {
                    *merge_with = (*merge_with + 1) % num as usize;
                }
            }
        }
    }

    fn test(
        dag_num: i32,
        mut before_merge_insertion: Vec<Interaction>,
        mut after_merge_insertion: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        preprocess(&mut before_merge_insertion, dag_num);
        preprocess(&mut after_merge_insertion, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as ClientID));
        }

        for interaction in before_merge_insertion {
            apply(interaction, &mut dags);
        }

        let (dag0,): (&mut TestDag,) = unsafe_array_mut_ref!(&mut dags, [0]);
        for dag in &dags[1..] {
            dag0.merge(dag);
        }

        dag0.push(1);
        let expected = dag0.frontier()[0];
        for dag in &mut dags[1..] {
            dag.merge(dag0);
        }
        for interaction in after_merge_insertion.iter_mut() {
            if let Some(merge) = interaction.merge_with {
                // odd dag merges with the odd
                // even dag merges with the even
                if merge % 2 != interaction.dag_idx % 2 {
                    interaction.merge_with = None;
                }
            }

            apply(*interaction, &mut dags);
        }

        let (dag0, dag1) = array_mut_ref!(&mut dags, [0, 1]);
        dag1.push(1);
        dag0.merge(dag1);
        // dbg!(dag0, dag1, expected);
        let actual = dags[0].get_common_ancestor(
            dags[0].nodes.get(&0).unwrap().last().unwrap().id,
            dags[1].nodes.get(&1).unwrap().last().unwrap().id,
        );
        prop_assert_eq!(actual.unwrap(), expected);
        Ok(())
    }

    fn apply(interaction: Interaction, dags: &mut [TestDag]) {
        let Interaction {
            dag_idx,
            len,
            merge_with,
        } = interaction;
        if let Some(merge_with) = merge_with {
            let (dag, merge_target): (&mut TestDag, &mut TestDag) =
                array_mut_ref!(dags, [dag_idx, merge_with]);
            dag.push(len);
            dag.merge(merge_target);
        } else {
            dags[dag_idx].push(len);
        }
    }
}
