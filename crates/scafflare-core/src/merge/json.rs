//! Structural three-way merge for JSON documents.
//!
//! Objects are merged key by key. Arrays and scalars are atomic values. When
//! no ancestor is available (`base == None`) the same algorithm degrades to a
//! two-way additive merge: keys present on only one side are kept and differing
//! values at the same pointer are conflicts.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonConflict {
    /// RFC 6901 JSON Pointer to the conflicting location (`""` is the root).
    pub pointer: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonMerge {
    Clean(Value),
    Conflicts(Vec<JsonConflict>),
}

pub fn merge3(base: Option<&Value>, ours: &Value, theirs: &Value) -> JsonMerge {
    let mut conflicts = Vec::new();
    let merged = merge_node(base, Some(ours), Some(theirs), "", &mut conflicts);
    if conflicts.is_empty() {
        JsonMerge::Clean(merged.unwrap_or(Value::Null))
    } else {
        JsonMerge::Conflicts(conflicts)
    }
}

fn merge_node(
    base: Option<&Value>,
    ours: Option<&Value>,
    theirs: Option<&Value>,
    pointer: &str,
    conflicts: &mut Vec<JsonConflict>,
) -> Option<Value> {
    if ours == theirs {
        return ours.cloned();
    }
    if base == ours {
        return theirs.cloned();
    }
    if base == theirs {
        return ours.cloned();
    }
    if let (Some(Value::Object(o)), Some(Value::Object(t))) = (ours, theirs) {
        let b = match base {
            Some(Value::Object(map)) => Some(map),
            _ => None,
        };
        let mut keys: Vec<&String> = o.keys().chain(t.keys()).collect();
        if let Some(b) = b {
            keys.extend(b.keys());
        }
        keys.sort();
        keys.dedup();
        let mut merged = serde_json::Map::new();
        for key in keys {
            let child = format!("{pointer}/{}", escape_token(key));
            let result = merge_node(
                b.and_then(|map| map.get(key)),
                o.get(key),
                t.get(key),
                &child,
                conflicts,
            );
            if let Some(value) = result {
                merged.insert(key.clone(), value);
            }
        }
        return Some(Value::Object(merged));
    }
    conflicts.push(JsonConflict {
        pointer: pointer.to_owned(),
    });
    None
}

fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn clean(base: Option<Value>, ours: Value, theirs: Value) -> Value {
        match merge3(base.as_ref(), &ours, &theirs) {
            JsonMerge::Clean(value) => value,
            JsonMerge::Conflicts(c) => panic!("unexpected conflicts {c:?}"),
        }
    }

    fn conflicts(base: Option<Value>, ours: Value, theirs: Value) -> Vec<String> {
        match merge3(base.as_ref(), &ours, &theirs) {
            JsonMerge::Clean(value) => panic!("expected conflicts, got {value}"),
            JsonMerge::Conflicts(c) => c.into_iter().map(|c| c.pointer).collect(),
        }
    }

    #[test]
    fn table_of_clean_merges() {
        let cases = [
            // (base, ours, theirs, expected)
            (json!({}), json!({}), json!({}), json!({})),
            (
                json!({"a":1}),
                json!({"a":1}),
                json!({"a":1}),
                json!({"a":1}),
            ),
            (
                json!({"a":1}),
                json!({"a":1,"u":2}),
                json!({"a":1,"t":3}),
                json!({"a":1,"u":2,"t":3}),
            ),
            (
                json!({"a":1}),
                json!({"a":2}),
                json!({"a":1}),
                json!({"a":2}),
            ),
            (
                json!({"a":1}),
                json!({"a":1}),
                json!({"a":2}),
                json!({"a":2}),
            ),
            (
                json!({"a":1}),
                json!({"a":2}),
                json!({"a":2}),
                json!({"a":2}),
            ),
            (
                json!({"a":{"x":1}}),
                json!({"a":{"x":1,"u":1}}),
                json!({"a":{"x":1,"t":1}}),
                json!({"a":{"x":1,"u":1,"t":1}}),
            ),
            (
                json!({"a":1,"b":2}),
                json!({"b":2}),
                json!({"a":1,"b":2}),
                json!({"b":2}),
            ),
            (
                json!({"a":1,"b":2}),
                json!({"a":1,"b":2}),
                json!({"b":2}),
                json!({"b":2}),
            ),
            (
                json!({"a":1,"b":2}),
                json!({"b":2}),
                json!({"b":2}),
                json!({"b":2}),
            ),
            (
                json!({"a":[1,2]}),
                json!({"a":[1,2,3]}),
                json!({"a":[1,2]}),
                json!({"a":[1,2,3]}),
            ),
            (
                json!({"a":[1]}),
                json!({"a":[9]}),
                json!({"a":[9]}),
                json!({"a":[9]}),
            ),
            (
                json!({"a":1}),
                json!({"a":1}),
                json!({"a":{"n":1}}),
                json!({"a":{"n":1}}),
            ),
            (
                json!({"s":{"test":"vitest"}}),
                json!({"s":{"test":"vitest","dev":"x"}}),
                json!({"s":{"test":"vitest","lint":"biome"}}),
                json!({"s":{"test":"vitest","dev":"x","lint":"biome"}}),
            ),
            (
                json!({"d":{"zod":"^3"}}),
                json!({"d":{"zod":"^3","dayjs":"^1"}}),
                json!({"d":{"zod":"^3","pino":"^9"}}),
                json!({"d":{"zod":"^3","dayjs":"^1","pino":"^9"}}),
            ),
            (
                json!({"a":null}),
                json!({"a":null}),
                json!({"a":1}),
                json!({"a":1}),
            ),
            (
                json!({"a":"x"}),
                json!("whole"),
                json!({"a":"x"}),
                json!("whole"),
            ),
        ];
        for (index, (base, ours, theirs, expected)) in cases.into_iter().enumerate() {
            assert_eq!(clean(Some(base), ours, theirs), expected, "case {index}");
        }
    }

    #[test]
    fn table_of_conflicts() {
        let cases = [
            (json!({"a":1}), json!({"a":2}), json!({"a":3}), vec!["/a"]),
            (
                json!({"a":{"x":1}}),
                json!({"a":{"x":2}}),
                json!({"a":{"x":3}}),
                vec!["/a/x"],
            ),
            (json!({"a":1}), json!({}), json!({"a":2}), vec!["/a"]),
            (json!({"a":1}), json!({"a":2}), json!({}), vec!["/a"]),
            (
                json!({"a":[1]}),
                json!({"a":[2]}),
                json!({"a":[3]}),
                vec!["/a"],
            ),
            (
                json!({"a":1}),
                json!({"a":{"n":1}}),
                json!({"a":2}),
                vec!["/a"],
            ),
            (
                json!({"a":1,"b":1}),
                json!({"a":2,"b":2}),
                json!({"a":3,"b":3}),
                vec!["/a", "/b"],
            ),
            (
                json!({"a/b":{"c~d":1}}),
                json!({"a/b":{"c~d":2}}),
                json!({"a/b":{"c~d":3}}),
                vec!["/a~1b/c~0d"],
            ),
            (json!(1), json!(2), json!(3), vec![""]),
        ];
        for (index, (base, ours, theirs, expected)) in cases.into_iter().enumerate() {
            assert_eq!(
                conflicts(Some(base), ours, theirs),
                expected,
                "case {index}"
            );
        }
    }

    #[test]
    fn two_way_additive_merge_without_base() {
        assert_eq!(
            clean(None, json!({"a":1,"u":1}), json!({"a":1,"t":1})),
            json!({"a":1,"u":1,"t":1})
        );
        assert_eq!(
            clean(None, json!({"s":{"x":"1"}}), json!({"s":{"y":"2"}})),
            json!({"s":{"x":"1","y":"2"}})
        );
        assert_eq!(conflicts(None, json!({"a":1}), json!({"a":2})), vec!["/a"]);
    }

    #[test]
    fn output_is_deterministic() {
        let a = merge3(
            Some(&json!({"k":1})),
            &json!({"z":1,"k":1}),
            &json!({"y":1,"k":1}),
        );
        let b = merge3(
            Some(&json!({"k":1})),
            &json!({"z":1,"k":1}),
            &json!({"y":1,"k":1}),
        );
        assert_eq!(a, b);
    }

    mod properties {
        use proptest::prelude::*;
        use serde_json::Value;

        use super::super::*;

        fn arb_value() -> impl Strategy<Value = Value> {
            let leaf = prop_oneof![
                Just(Value::Null),
                any::<bool>().prop_map(Value::Bool),
                (0i64..4).prop_map(Value::from),
                "[a-c]{0,2}".prop_map(Value::String),
            ];
            leaf.prop_recursive(3, 24, 4, |inner| {
                prop_oneof![
                    proptest::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
                    proptest::collection::btree_map("[a-d/~]{1,2}", inner, 0..4)
                        .prop_map(|m| Value::Object(m.into_iter().collect())),
                ]
            })
        }

        proptest! {
            #[test]
            fn unchanged_side_yields_other_side(base in arb_value(), other in arb_value()) {
                prop_assert_eq!(merge3(Some(&base), &base, &other), JsonMerge::Clean(other.clone()));
                prop_assert_eq!(merge3(Some(&base), &other, &base), JsonMerge::Clean(other.clone()));
                prop_assert_eq!(merge3(Some(&base), &other, &other), JsonMerge::Clean(other));
            }

            #[test]
            fn merge_is_symmetric(base in arb_value(), ours in arb_value(), theirs in arb_value()) {
                let forward = merge3(Some(&base), &ours, &theirs);
                let backward = merge3(Some(&base), &theirs, &ours);
                prop_assert_eq!(forward, backward);
            }

            #[test]
            fn two_way_merge_of_equal_documents_is_identity(value in arb_value()) {
                prop_assert_eq!(merge3(None, &value, &value), JsonMerge::Clean(value));
            }

            #[test]
            fn disjoint_key_additions_always_merge(
                base in proptest::collection::btree_map("[a-c]", 0i64..3, 0..3),
                ours_extra in proptest::collection::btree_map("o[a-c]", 0i64..3, 0..3),
                theirs_extra in proptest::collection::btree_map("t[a-c]", 0i64..3, 0..3),
            ) {
                let to_value = |extra: &std::collections::BTreeMap<String, i64>| {
                    let mut map: serde_json::Map<String, Value> =
                        base.iter().map(|(k, v)| (k.clone(), Value::from(*v))).collect();
                    map.extend(extra.iter().map(|(k, v)| (k.clone(), Value::from(*v))));
                    Value::Object(map)
                };
                let base_value = to_value(&Default::default());
                let merged = merge3(Some(&base_value), &to_value(&ours_extra), &to_value(&theirs_extra));
                let mut expected = base_value.as_object().unwrap().clone();
                expected.extend(ours_extra.iter().map(|(k, v)| (k.clone(), Value::from(*v))));
                expected.extend(theirs_extra.iter().map(|(k, v)| (k.clone(), Value::from(*v))));
                prop_assert_eq!(merged, JsonMerge::Clean(Value::Object(expected)));
            }
        }
    }
}
