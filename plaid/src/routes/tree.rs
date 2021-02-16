use std::{cmp::Ordering, collections::HashMap, hash::Hash};

use crate::WrappedHandler;

pub struct RouteTree<G, L, E>
where
    G: 'static,
    L: 'static,
{
    root: StaticNode<G, L, E>,
}

struct InsertionContext<'a, G, L, E>
where
    G: 'static,
    L: 'static,
{
    parts: std::str::Split<'a, char>,
    methods: Vec<hyper::Method>,
    handler: WrappedHandler<G, L, E>,
}

impl<G, L, E> RouteTree<G, L, E> {
    pub fn new() -> Self {
        RouteTree {
            root: StaticNode {
                // path: String::new(),
                priority: 0,
                static_children: StaticChildren::new(),
                param_children: ParameterChildren::new(),
                routes: MethodMap::new(),
            },
        }
    }

    pub fn add_route(
        &mut self,
        methods: &[hyper::Method],
        path: &'static str,
        handler: WrappedHandler<G, L, E>,
    ) {
        let path = path.trim_matches('/');

        let mut ctx = InsertionContext {
            parts: path.split('/'),
            methods: methods.to_vec(),
            handler,
        };
        if path.is_empty() {
            // Set this as the root
            self.root.set(&ctx);
        } else {
            // Insert as child of the root
            self.root.insert(&mut ctx);
        }
    }

    // pub fn add_tree(&mut self, tree: RouteTree<R>) {
    //     unimplemented!()
    // }

    pub fn route_to(&self, path: &str) -> Option<(&MethodMap<G, L, E>, RouteParameters)> {
        let mut params = RouteParameters::new();
        if path.is_empty() {
            Some(&self.root.routes)
        } else {
            let mut path_parts = path.split('/');
            self.root.find(&mut path_parts, &mut params)
        }
        .map(|method_map| (method_map, params))
    }
}

// TODO: maybe just make this a list of tuples with lookup by method?
// since it'll never be big enough to need a hash
type MethodMap<G, L, E> = HashMap<hyper::Method, WrappedHandler<G, L, E>>;

#[derive(Debug)]
pub struct RouteParameters {
    pub named: HashMap<String, Parameter>,
    pub ordered: Vec<Parameter>,
}

impl RouteParameters {
    pub(crate) fn new() -> Self {
        Self {
            named: HashMap::new(),
            ordered: Vec::new(),
        }
    }

    pub fn first_i32(&self) -> Option<i32> {
        if let Some(Parameter::I32(first)) = self.ordered.first() {
            Some(*first)
        } else {
            None
        }
    }

    pub fn get_i32(&self, name: &str) -> Option<i32> {
        if let Some(Parameter::I32(value)) = self.named.get(name) {
            Some(*value)
        } else {
            None
        }
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        if let Some(Parameter::String(value)) = self.named.get(name) {
            Some(value.clone())
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub enum Parameter {
    String(String),
    U32(u32),
    I32(i32),
}

struct StaticNode<G, L, E>
where
    G: 'static,
    L: 'static,
{
    // path: String,
    priority: usize,
    static_children: StaticChildren<G, L, E>,
    param_children: ParameterChildren<G, L, E>,
    routes: MethodMap<G, L, E>,
}

impl<G, L, E> StaticNode<G, L, E> {
    fn new() -> Self {
        Self {
            priority: 0,
            static_children: StaticChildren::new(),
            param_children: ParameterChildren::new(),
            routes: MethodMap::new(),
        }
    }
    fn insert(&mut self, ctx: &mut InsertionContext<G, L, E>) {
        // Bump priority on insertion
        self.priority += 1;

        if let Some(next_part) = ctx.parts.next() {
            // If there is another path part, insert into the correct child or
            // generate a new one.
            if next_part.starts_with(':') {
                // Parameter Nodematch
                self.param_children.insert(next_part, ctx);
            } else {
                self.static_children.insert(next_part, ctx);
            }
        } else {
            // If there are no more parts, this is the match. Add a leaf by
            // method.
            self.set(ctx)
        }
    }

    fn set(&mut self, ctx: &InsertionContext<G, L, E>) {
        for method in ctx.methods.clone() {
            self.routes.insert(method, ctx.handler.clone());
        }
    }

    fn find(
        &self,
        path_parts: &mut std::str::Split<char>,
        params: &mut RouteParameters,
    ) -> Option<&MethodMap<G, L, E>> {
        if let Some(next_part) = path_parts.next() {
            // Try to find an appropriate child node
            if let Some(node) = self.static_children.find(next_part) {
                node.find(path_parts, params)
            } else if let Some(node) = self.param_children.find(next_part, params) {
                node.find(path_parts, params)
            } else {
                None
            }
        } else {
            // Else, exhausted parts and this is the endpoint
            Some(&self.routes)
        }
    }
}

/// Parse the type of the paramter (default is String).
/// Type hinting can be done by /users/:id{i32}/profile
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
enum ParameterType {
    U32,
    I32,
    String,
}

// String always last (Derive Ord/PartialOrd gives lexical ordering)
impl PartialOrd for ParameterType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ParameterType {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (ParameterType::String, ParameterType::String) => Ordering::Equal,
            (ParameterType::String, _) => Ordering::Greater,
            (_, ParameterType::String) => Ordering::Less,
            (_, _) => Ordering::Equal,
        }
    }
}

impl From<&str> for ParameterType {
    fn from(s: &str) -> Self {
        let lb = s.find('{');
        let rb = s.find('}');

        match (lb, rb) {
            (Some(lb), Some(rb)) => match s.get((lb + 1)..rb) {
                Some("u32") => ParameterType::U32,
                Some("i32") => ParameterType::I32,
                _ => ParameterType::String,
            },
            _ => ParameterType::String,
        }
    }
}

impl ParameterType {
    // TODO: could be better, but this happens during insert/setup, not
    // runtime, so it's not a huge deal
    fn parse_name(name: &str) -> (String, Self) {
        let name = name.trim_start_matches(':');
        let lb = name.find('{');
        let rb = name.find('}');

        let name_part = if let Some(lb) = lb {
            if let Some(n) = name.get(0..lb) {
                n
            } else {
                name
            }
        } else {
            name
        };

        let ptype = match (lb, rb) {
            (Some(lb), Some(rb)) => match name.get((lb + 1)..rb) {
                Some("u32") => ParameterType::U32,
                Some("i32") => ParameterType::I32,
                _ => ParameterType::String,
            },
            _ => ParameterType::String,
        };

        (String::from(name_part), ptype)
    }
}

struct ParameterNode<G, L, E>
where
    G: 'static,
    L: 'static,
{
    name: Option<String>,
    ptype: ParameterType,
    static_children: StaticChildren<G, L, E>,
    param_children: ParameterChildren<G, L, E>,
    routes: MethodMap<G, L, E>,
}

impl<G, L, E> ParameterNode<G, L, E> {
    // fn new(name: Option<String>) -> Self {
    //     Self {
    //         name,
    //         ptype: ParameterType::String,
    //         static_children: StaticChildren::new(),
    //         param_children: ParameterChildren::new(),
    //         routes: MethodMap::new(),
    //     }
    // }

    fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    fn insert(&mut self, ctx: &mut InsertionContext<G, L, E>) {
        if let Some(next_part) = ctx.parts.next() {
            // If there is another path part, insert into the correct child or
            // generate a new one.
            if next_part.starts_with(':') {
                self.param_children.insert(next_part, ctx);
            } else {
                self.static_children.insert(next_part, ctx);
            }
        } else {
            // If there are no more parts, this is the match. Add a leaf by
            // method.
            self.set(ctx)
        }
    }

    fn set(&mut self, ctx: &InsertionContext<G, L, E>) {
        for method in ctx.methods.clone() {
            self.routes.insert(method, ctx.handler.clone());
        }
    }

    fn find(
        &self,
        path_parts: &mut std::str::Split<char>,
        params: &mut RouteParameters,
    ) -> Option<&MethodMap<G, L, E>> {
        if let Some(next_part) = path_parts.next() {
            // Try to find an appropriate child node
            if let Some(node) = self.static_children.find(next_part) {
                node.find(path_parts, params)
            } else if let Some(node) = self.param_children.find(next_part, params) {
                node.find(path_parts, params)
            } else {
                None
            }
        } else {
            // Else, exhausted parts and this is the endpoint
            Some(&self.routes)
        }
    }
}

/// NodeChildren holds either a vec or hashmap of child nodes (static nodes)
/// with an optional parameter node
enum Children<K, V>
where
    K: Hash,
{
    Few(Vec<(K, Box<V>)>),
    Many(HashMap<K, Box<V>>),
}

type StaticChildren<G, L, E> = Children<String, StaticNode<G, L, E>>;
type ParameterChildren<G, L, E> = Children<ParameterType, ParameterNode<G, L, E>>;

const NODE_CHILDREN_VEC_LIMIT: usize = 15;

impl<K, V> Children<K, V>
where
    K: Eq + Hash,
{
    fn new() -> Self {
        Children::<K, V>::Few(Vec::new())
    }

    // fn len(&self) -> usize {
    //     match self {
    //         Children::Few(ref v) => v.len(),
    //         Children::Many(ref h) => h.len(),
    //     }
    // }

    fn rebalance(self) -> Self {
        if let Children::Few(v) = self {
            Children::Many(v.into_iter().collect::<HashMap<K, Box<V>>>())
        // let mut map = HashMap::new();
        // for (name, node) in v.into_iter() {
        //     let _ = map.insert(name, node);
        // }
        // *self = Children::Many(v);
        } else {
            self
        }
    }
}

impl<G, L, E> StaticChildren<G, L, E> {
    fn find(&self, path: &str) -> Option<&Box<StaticNode<G, L, E>>> {
        match self {
            // Linear search of Vec<Node>
            Children::Few(ref v) => {
                for (node_path, node) in v.iter() {
                    if path == node_path {
                        return Some(node);
                    }
                }
                None
            }
            // Lookup of HashMap<Node>
            Children::Many(ref h) => h.get(path),
        }
    }

    fn find_mut(&mut self, path: &str) -> Option<&mut StaticNode<G, L, E>> {
        match self {
            Children::Few(v) => {
                for (node_path, node) in v.iter_mut() {
                    if path == node_path {
                        return Some(node.as_mut());
                    }
                }
                None
            }
            Children::Many(h) => h.get_mut(path).map(|boxed| boxed.as_mut()),
        }
    }

    fn insert(&mut self, path: &str, ctx: &mut InsertionContext<G, L, E>) {
        // Check if a child with this path exists
        if let Some(node) = self.find_mut(path) {
            // This is a child of the given node
            node.insert(ctx);
        } else {
            // This is a new child
            // Check if we need to rebalance
            // TODO: rebalancing
            // if let Children::Few(v) = self {
            //     if v.len() >= NODE_CHILDREN_VEC_LIMIT {
            //         *self = self.rebalance();
            //     }
            // }

            // Add the node
            let mut new_node = Box::new(StaticNode::new());
            new_node.insert(ctx);
            match self {
                Children::Few(v) => {
                    v.push((String::from(path), new_node));

                    // Sort nodes by priority (descending)
                    v.sort_by(|(_, a), (_, b)| a.priority.cmp(&b.priority).reverse());
                }
                Children::Many(h) => {
                    h.insert(String::from(path), new_node);
                }
            }
        }
    }
}

impl<G, L, E> ParameterChildren<G, L, E> {
    fn find(
        &self,
        path: &str,
        params: &mut RouteParameters,
    ) -> Option<&Box<ParameterNode<G, L, E>>> {
        match self {
            Children::Few(v) => v.iter().find_map(|(ptype, node)| {
                if self.find_loop(path, ptype, node, params) {
                    Some(node)
                } else {
                    None
                }
            }),
            Children::Many(h) => h.iter().find_map(|(ptype, node)| {
                if self.find_loop(path, ptype, node, params) {
                    Some(node)
                } else {
                    None
                }
            }),
        }
    }

    fn find_loop(
        &self,
        path: &str,
        ptype: &ParameterType,
        node: &Box<ParameterNode<G, L, E>>,
        params: &mut RouteParameters,
    ) -> bool {
        if let Some(value) = match ptype {
            ParameterType::U32 => path.parse::<u32>().ok().map(Parameter::U32),
            ParameterType::I32 => path.parse::<i32>().ok().map(Parameter::I32),
            ParameterType::String => Some(Parameter::String(String::from(path))),
        } {
            if let Some(name) = node.name.clone() {
                params.ordered.push(value.clone());
                params.named.insert(name, value);
            } else {
                params.ordered.push(value.clone());
            }
            true
        } else {
            false
        }
    }

    fn insert(&mut self, path: &str, ctx: &mut InsertionContext<G, L, E>) {
        // Try parsing the name for a type
        let (param_name, param_type) = ParameterType::parse_name(path);
        if let Some(node) = match self {
            Children::Few(v) => v
                .iter_mut()
                .find(|(t, _)| *t == param_type)
                .map(|(_, b)| b.as_mut()),
            Children::Many(h) => h.get_mut(&param_type).map(|boxed| boxed.as_mut()),
        } {
            if let Some(n) = node.name.clone() {
                if param_name != n {
                    // TODO: Better error here
                    panic!(
                        "Parameter route defined with overlapping names and same type /{}, /{}",
                        param_name, n
                    );
                }
            } else {
                node.set_name(Some(param_name));
            }
            node.insert(ctx);
        } else {
            // This is a new child
            // Check if we need to rebalance
            // TODO: rebalancing
            // if let Children::Few(v) = self {
            //     if v.len() >= NODE_CHILDREN_VEC_LIMIT {
            //         self.rebalance();
            //     }
            // }

            // Add the node
            let mut new_node = Box::new(ParameterNode {
                name: Some(param_name),
                ptype: param_type.clone(),
                static_children: StaticChildren::new(),
                param_children: ParameterChildren::new(),
                routes: MethodMap::new(),
            });
            new_node.insert(ctx);
            match self {
                Children::Few(v) => {
                    v.push((param_type, new_node));

                    // Sort nodes by type (so String is always last)
                    v.sort_by(|(_, a), (_, b)| a.ptype.cmp(&b.ptype));
                }
                Children::Many(h) => {
                    h.insert(param_type, new_node);
                }
            }
        }
    }
}
