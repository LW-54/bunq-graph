use clepsydra_lib::RuntimeEvaluator;
use thiserror::Error;

use bunq_graph::AccountBalance;

#[cfg(feature = "embedded-graph")]
use clepsydra_lib::clepsydra;

#[cfg(feature = "embedded-graph")]
clepsydra!(EmbeddedGraph, "graph.toml");

#[derive(Debug, Error)]
pub enum GraphError {
    #[cfg(not(feature = "embedded-graph"))]
    #[error("no embedded graph was compiled; provide --graph or BUNQ_GRAPH")]
    MissingGraph,
    #[error("could not load graph configuration: {0}")]
    Config(#[from] clepsydra_lib::ConfigError),
    #[error("graph evaluation failed: {0}")]
    Evaluate(#[from] clepsydra_lib::EvalError),
}

pub enum Graph {
    #[cfg(feature = "embedded-graph")]
    Embedded,
    Runtime {
        evaluator: Box<RuntimeEvaluator<'static>>,
    },
}

impl Graph {
    pub fn load(graph_config: Option<String>) -> Result<Self, GraphError> {
        match graph_config {
            Some(input) => {
                let config = clepsydra_lib::clepsydra_core::config::Config::from_input(&input)?;
                Ok(Self::Runtime {
                    evaluator: Box::new(clepsydra_lib::clepsydra(&config)?),
                })
            }
            None => {
                #[cfg(feature = "embedded-graph")]
                return Ok(Self::Embedded);
                #[cfg(not(feature = "embedded-graph"))]
                return Err(GraphError::MissingGraph);
            }
        }
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        match self {
            #[cfg(feature = "embedded-graph")]
            Self::Embedded => EmbeddedGraph::NAMES.to_vec(),
            Self::Runtime { evaluator } => evaluator.names().iter().map(String::as_str).collect(),
        }
    }

    pub fn eval<'account>(
        &self,
        initial: &'account [AccountBalance<'account>],
    ) -> Result<Vec<AccountBalance<'account>>, GraphError> {
        match self {
            #[cfg(feature = "embedded-graph")]
            Self::Embedded => EmbeddedGraph::vec_eval(initial).map_err(GraphError::from),
            Self::Runtime { evaluator } => evaluator.vec_eval(initial).map_err(GraphError::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Graph;

    #[test]
    fn evaluates_runtime_graph_with_dynamic_accounts() {
        let source = r#"
            [accounts.checking]
            [accounts.reserve]
            [[transfers]]
            source = "checking"
            target = "reserve"
            volume = 25
        "#;
        let graph = Graph::load(Some(source.to_owned())).expect("valid graph");
        let initial = [("checking", 100), ("reserve", 0)];
        let target = graph.eval(&initial).expect("runtime graph evaluates");

        assert_eq!(target, [("checking", 75), ("reserve", 25)]);
    }
}
