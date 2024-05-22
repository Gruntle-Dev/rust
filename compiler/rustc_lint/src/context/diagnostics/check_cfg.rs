use rustc_middle::bug;
use rustc_session::{config::ExpectedValues, Session};
use rustc_span::edit_distance::find_best_match_for_name;
use rustc_span::{sym, Span, Symbol};

use crate::lints;

const MAX_CHECK_CFG_NAMES_OR_VALUES: usize = 35;

fn sort_and_truncate_possibilities(
    sess: &Session,
    mut possibilities: Vec<Symbol>,
) -> (Vec<Symbol>, usize) {
    let n_possibilities = if sess.opts.unstable_opts.check_cfg_all_expected {
        possibilities.len()
    } else {
        std::cmp::min(possibilities.len(), MAX_CHECK_CFG_NAMES_OR_VALUES)
    };

    possibilities.sort_by(|s1, s2| s1.as_str().cmp(s2.as_str()));

    let and_more = possibilities.len().saturating_sub(n_possibilities);
    possibilities.truncate(n_possibilities);
    (possibilities, and_more)
}

enum EscapeQuotes {
    Yes,
    No,
}

fn to_check_cfg_arg(name: Symbol, value: Option<Symbol>, quotes: EscapeQuotes) -> String {
    if let Some(value) = value {
        let value = str::escape_debug(value.as_str()).to_string();
        let values = match quotes {
            EscapeQuotes::Yes => format!("\\\"{}\\\"", value.replace("\"", "\\\\\\\\\"")),
            EscapeQuotes::No => format!("\"{value}\""),
        };
        format!("cfg({name}, values({values}))")
    } else {
        format!("cfg({name})")
    }
}

pub(super) fn unexpected_cfg_name(
    sess: &Session,
    (name, name_span): (Symbol, Span),
    value: Option<(Symbol, Span)>,
) -> lints::UnexpectedCfgName {
    #[allow(rustc::potential_query_instability)]
    let possibilities: Vec<Symbol> = sess.psess.check_config.expecteds.keys().copied().collect();

    let mut names_possibilities: Vec<_> = if value.is_none() {
        // We later sort and display all the possibilities, so the order here does not matter.
        #[allow(rustc::potential_query_instability)]
        sess.psess
            .check_config
            .expecteds
            .iter()
            .filter_map(|(k, v)| match v {
                ExpectedValues::Some(v) if v.contains(&Some(name)) => Some(k),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };

    let is_from_cargo = rustc_session::utils::was_invoked_from_cargo();
    let mut is_feature_cfg = name == sym::feature;

    let code_sugg = if is_feature_cfg && is_from_cargo {
        lints::unexpected_cfg_name::CodeSuggestion::DefineFeatures
    // Suggest the most probable if we found one
    } else if let Some(best_match) = find_best_match_for_name(&possibilities, name, None) {
        is_feature_cfg |= best_match == sym::feature;

        if let Some(ExpectedValues::Some(best_match_values)) =
            sess.psess.check_config.expecteds.get(&best_match)
        {
            // We will soon sort, so the initial order does not matter.
            #[allow(rustc::potential_query_instability)]
            let mut possibilities = best_match_values.iter().flatten().collect::<Vec<_>>();
            possibilities.sort_by_key(|s| s.as_str());

            let get_possibilities_sub = || {
                if !possibilities.is_empty() {
                    let possibilities =
                        possibilities.iter().copied().cloned().collect::<Vec<_>>().into();
                    Some(lints::unexpected_cfg_name::ExpectedValues { best_match, possibilities })
                } else {
                    None
                }
            };

            if let Some((value, value_span)) = value {
                if best_match_values.contains(&Some(value)) {
                    lints::unexpected_cfg_name::CodeSuggestion::SimilarNameAndValue {
                        span: name_span,
                        code: best_match.to_string(),
                    }
                } else if best_match_values.contains(&None) {
                    lints::unexpected_cfg_name::CodeSuggestion::SimilarNameNoValue {
                        span: name_span.to(value_span),
                        code: best_match.to_string(),
                    }
                } else if let Some(first_value) = possibilities.first() {
                    lints::unexpected_cfg_name::CodeSuggestion::SimilarNameDifferentValues {
                        span: name_span.to(value_span),
                        code: format!("{best_match} = \"{first_value}\""),
                        expected: get_possibilities_sub(),
                    }
                } else {
                    lints::unexpected_cfg_name::CodeSuggestion::SimilarNameDifferentValues {
                        span: name_span.to(value_span),
                        code: best_match.to_string(),
                        expected: get_possibilities_sub(),
                    }
                }
            } else {
                lints::unexpected_cfg_name::CodeSuggestion::SimilarName {
                    span: name_span,
                    code: best_match.to_string(),
                    expected: get_possibilities_sub(),
                }
            }
        } else {
            lints::unexpected_cfg_name::CodeSuggestion::SimilarName {
                span: name_span,
                code: best_match.to_string(),
                expected: None,
            }
        }
    } else {
        let similar_values = if !names_possibilities.is_empty() && names_possibilities.len() <= 3 {
            names_possibilities.sort();
            names_possibilities
                .iter()
                .map(|cfg_name| lints::unexpected_cfg_name::FoundWithSimilarValue {
                    span: name_span,
                    code: format!("{cfg_name} = \"{name}\""),
                })
                .collect()
        } else {
            vec![]
        };
        let expected_names = if !possibilities.is_empty() {
            let (possibilities, and_more) = sort_and_truncate_possibilities(sess, possibilities);
            Some(lints::unexpected_cfg_name::ExpectedNames {
                possibilities: possibilities.into(),
                and_more,
            })
        } else {
            None
        };
        lints::unexpected_cfg_name::CodeSuggestion::SimilarValues {
            with_similar_values: similar_values,
            expected_names,
        }
    };

    let inst = |escape_quotes| to_check_cfg_arg(name, value.map(|(v, _s)| v), escape_quotes);

    let invocation_help = if is_from_cargo {
        let sub = if !is_feature_cfg {
            Some(lints::UnexpectedCfgCargoHelp::new(
                &inst(EscapeQuotes::No),
                &inst(EscapeQuotes::Yes),
            ))
        } else {
            None
        };
        lints::unexpected_cfg_name::InvocationHelp::Cargo { sub }
    } else {
        lints::unexpected_cfg_name::InvocationHelp::Rustc(lints::UnexpectedCfgRustcHelp::new(
            &inst(EscapeQuotes::No),
        ))
    };

    lints::UnexpectedCfgName { code_sugg, invocation_help, name }
}

pub(super) fn unexpected_cfg_value(
    sess: &Session,
    (name, name_span): (Symbol, Span),
    value: Option<(Symbol, Span)>,
) -> lints::UnexpectedCfgValue {
    let Some(ExpectedValues::Some(values)) = &sess.psess.check_config.expecteds.get(&name) else {
        bug!(
            "it shouldn't be possible to have a diagnostic on a value whose name is not in values"
        );
    };
    let mut have_none_possibility = false;
    // We later sort possibilities if it is not empty, so the
    // order here does not matter.
    #[allow(rustc::potential_query_instability)]
    let possibilities: Vec<Symbol> = values
        .iter()
        .inspect(|a| have_none_possibility |= a.is_none())
        .copied()
        .flatten()
        .collect();
    let is_from_cargo = rustc_session::utils::was_invoked_from_cargo();

    // Show the full list if all possible values for a given name, but don't do it
    // for names as the possibilities could be very long
    let code_sugg = if !possibilities.is_empty() {
        let expected_values = {
            let (possibilities, and_more) =
                sort_and_truncate_possibilities(sess, possibilities.clone());
            lints::unexpected_cfg_value::ExpectedValues {
                name,
                have_none_possibility,
                possibilities: possibilities.into(),
                and_more,
            }
        };

        let suggestion = if let Some((value, value_span)) = value {
            // Suggest the most probable if we found one
            if let Some(best_match) = find_best_match_for_name(&possibilities, value, None) {
                Some(lints::unexpected_cfg_value::ChangeValueSuggestion::SimilarName {
                    span: value_span,
                    best_match,
                })
            } else {
                None
            }
        } else if let &[first_possibility] = &possibilities[..] {
            Some(lints::unexpected_cfg_value::ChangeValueSuggestion::SpecifyValue {
                span: name_span.shrink_to_hi(),
                first_possibility,
            })
        } else {
            None
        };

        lints::unexpected_cfg_value::CodeSuggestion::ChangeValue { expected_values, suggestion }
    } else if have_none_possibility {
        let suggestion =
            value.map(|(_value, value_span)| lints::unexpected_cfg_value::RemoveValueSuggestion {
                span: name_span.shrink_to_hi().to(value_span),
            });
        lints::unexpected_cfg_value::CodeSuggestion::RemoveValue { suggestion, name }
    } else {
        let span = if let Some((_value, value_span)) = value {
            name_span.to(value_span)
        } else {
            name_span
        };
        let suggestion = lints::unexpected_cfg_value::RemoveConditionSuggestion { span };
        lints::unexpected_cfg_value::CodeSuggestion::RemoveCondition { suggestion, name }
    };

    // We don't want to suggest adding values to well known names
    // since those are defined by rustc it-self. Users can still
    // do it if they want, but should not encourage them.
    let is_cfg_a_well_know_name = sess.psess.check_config.well_known_names.contains(&name);

    let inst = |escape_quotes| to_check_cfg_arg(name, value.map(|(v, _s)| v), escape_quotes);

    let invocation_help = if is_from_cargo {
        let help = if name == sym::feature {
            if let Some((value, _value_span)) = value {
                Some(lints::unexpected_cfg_value::CargoHelp::AddFeature { value })
            } else {
                Some(lints::unexpected_cfg_value::CargoHelp::DefineFeatures)
            }
        } else if !is_cfg_a_well_know_name {
            Some(lints::unexpected_cfg_value::CargoHelp::Other(lints::UnexpectedCfgCargoHelp::new(
                &inst(EscapeQuotes::No),
                &inst(EscapeQuotes::Yes),
            )))
        } else {
            None
        };
        lints::unexpected_cfg_value::InvocationHelp::Cargo(help)
    } else {
        let help = if !is_cfg_a_well_know_name {
            Some(lints::UnexpectedCfgRustcHelp::new(&inst(EscapeQuotes::No)))
        } else {
            None
        };
        lints::unexpected_cfg_value::InvocationHelp::Rustc(help)
    };

    lints::UnexpectedCfgValue {
        code_sugg,
        invocation_help,
        has_value: value.is_some(),
        value: value.map_or_else(String::new, |(v, _span)| v.to_string()),
    }
}
