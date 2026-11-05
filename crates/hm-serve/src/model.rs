//! The map the server holds: what `hm_layout::Map::build` made of the facts, plus the
//! counts the overview shows next to it.

use hm_index::index::Result;
use hm_index::view::map_facts;
use hm_layout::{InFile, InLink, Input, Map};
use hm_store::Store;

/// One build of everything the views are cut from. Swapped as a whole when the facts
/// change, so a request never sees half of an old map and half of a new one.
#[derive(Debug, Clone)]
pub struct Model {
    pub map: Map,
    /// Files left off the map, as (class, count), sorted by class.
    pub excluded: Vec<(String, u32)>,
    /// Files on the map.
    pub files_total: u32,
}

impl Model {
    /// Reads the map facts from `store` and builds the map; the root box is named
    /// `project_name`. `previous` is the model of the last build in this run, so the
    /// layout can keep positions steady.
    pub fn load(store: &Store, project_name: &str, previous: Option<&Model>) -> Result<Model> {
        let facts = map_facts(store)?;
        let input = Input {
            files: facts
                .files
                .iter()
                .map(|f| InFile {
                    path: f.path.clone(),
                    loc: f.loc,
                    generated: f.generated,
                })
                .collect(),
            links: facts
                .links
                .iter()
                .map(|l| InLink {
                    src: l.src,
                    dst: l.dst,
                    refs: l.refs,
                })
                .collect(),
        };
        // A `BTreeMap` iterates by key, which is the class.
        let excluded = facts.excluded.into_iter().collect();
        Ok(Model::build(&input, project_name, excluded, previous))
    }

    fn build(
        input: &Input,
        project_name: &str,
        excluded: Vec<(String, u32)>,
        previous: Option<&Model>,
    ) -> Model {
        let map = Map::build(input, project_name, previous.map(|m| &m.map));
        Model {
            map,
            excluded,
            files_total: input.files.len() as u32,
        }
    }

    /// A model of a ready-made [`Input`], with the root box named `shop`. For tests
    /// that need a project too large to index, such as the payload budgets.
    #[doc(hidden)]
    pub fn from_input(input: &Input, excluded: Vec<(String, u32)>) -> Model {
        Model::build(input, "shop", excluded, None)
    }
}
