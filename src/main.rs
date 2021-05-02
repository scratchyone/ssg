use html_parser::{Dom, Element, Node};
use log::*;
use pretty_env_logger;
use regex::Regex;
use rusty_v8 as v8;
use std::fs;
use std::{collections::HashMap, path::PathBuf, str::FromStr};

fn parse_html(real_path: PathBuf) -> Dom {
    trace!("Starting HTML parse");
    Dom::parse(&fs::read_to_string(real_path).unwrap()).unwrap()
}
fn get_component_html(element: Dom) -> Node {
    element
        .children
        .into_iter()
        .find(|t| match t {
            Node::Element(e) if e.name == "component" => true,
            _ => false,
        })
        .expect("Expected outer <component> element")
        .clone()
}
fn get_styles(element: Dom) -> Option<String> {
    match element.children.iter().find(|t| match t {
        Node::Element(e) if e.name == "style" => true,
        _ => false,
    }) {
        Some(t) => match t {
            Node::Element(e) => Some(match &e.children[0] {
                html_parser::Node::Text(s) => s.clone(),
                _ => panic!("Failed to parse style tag"),
            }),
            _ => panic!("Failed to parse style tag"),
        },
        None => None,
    }
}

fn main() {
    pretty_env_logger::init();
    debug!("Initializing V8 platform");
    let platform = v8::new_default_platform().unwrap();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    let component_files = fs::read_dir("./components").unwrap();
    let mut components = HashMap::new();
    for path in component_files {
        let real_path = path.unwrap().path();
        debug!("Indexing: {}", real_path.display());
        let elements = parse_html(real_path.clone());
        let component_html = get_component_html(elements.clone());
        if let Node::Element(e) = component_html {
            components.insert(
                real_path.file_stem().unwrap().to_str().unwrap().to_string(),
                e.children,
            );
        } else {
            panic!();
        }
    }
    let path = PathBuf::from_str("./index.html").unwrap();
    info!("Processing: {}", path.display());
    trace!("Source: {}", fs::read_to_string(path.clone()).unwrap());
    let elements = parse_html(path.clone());
    let component_html = get_component_html(elements.clone());
    let component_html = match component_html {
        Node::Element(e) => Node::Element(Element {
            name: "body".into(),
            ..e
        }),
        other => other,
    };
    let subbed = sub_values(component_html.clone(), HashMap::new(), components);
    info!("Output: {}", node_tree_to_html(subbed));
}

fn sub_values(
    node: Node,
    map: HashMap<String, String>,
    components: HashMap<String, Vec<Node>>,
) -> Node {
    match node {
        Node::Text(t) => {
            let mut text = t;
            let re = Regex::new(r"\{(.*?)\}").unwrap();
            for cap in re.captures_iter(&text.clone()) {
                trace!("Evaluating: {}", &cap[1]);
                let isolate = &mut v8::Isolate::new(Default::default());
                let scope = &mut v8::HandleScope::new(isolate);
                let context = v8::Context::new(scope);
                let mut scope = &mut v8::ContextScope::new(scope, context);
                for (key, value) in map.clone() {
                    let key = v8::String::new(scope, &key).unwrap().into();
                    let value = v8::String::new(scope, &value).unwrap().into();
                    context
                        .global(&mut scope)
                        .set(&mut scope, key, value)
                        .unwrap();
                }
                let code = v8::String::new(scope, &cap[1]).unwrap();
                let script = v8::Script::compile(scope, code, None).unwrap();
                let result = script.run(scope).unwrap();
                let result = result.to_string(scope).unwrap().to_rust_string_lossy(scope);
                trace!("Result: {}", result);
                text = text.replace(&cap[0], &result);
            }
            Node::Text(text)
        }
        Node::Element(e) => Node::Element(Element {
            children: e
                .children
                .into_iter()
                .flat_map(|c| match c {
                    Node::Element(e) if components.contains_key(&e.name) => {
                        trace!("Inlining {:#?}", &e.name);
                        let mut map = HashMap::new();
                        for attr in e.attributes {
                            if let Some(val) = attr.1 {
                                map.insert(attr.0, val);
                            }
                        }
                        trace!("{} attrs: {:#?}", e.name, map);
                        components
                            .get(&e.name)
                            .unwrap()
                            .clone()
                            .into_iter()
                            .map(|tag| sub_values(tag, map.clone(), components.clone()))
                            .collect()
                    }
                    _ => vec![sub_values(c, map.clone(), components.clone())],
                })
                .collect(),
            ..e
        }),
        other => other,
    }
}

fn node_tree_to_html(node: Node) -> String {
    let mut buf = vec![];
    match &node {
        Node::Text(t) => buf.push(t.clone()),
        Node::Element(e) => {
            buf.push(format!("<{}", e.name));
            for attr in e.attributes.clone() {
                buf.push(format!(" {}", attr.0));
                if let Some(v) = attr.1 {
                    buf.push(format!("=\"{}\"", v));
                }
            }
            buf.push(">".into());
        }
        _ => {}
    }
    if let Node::Element(e) = node {
        for child in e.children {
            buf.push(node_tree_to_html(child));
        }
        buf.push(format!("</{}>", e.name));
    }
    buf.join("")
}
