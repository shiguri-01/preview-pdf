use std::collections::{HashMap, HashSet};

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{
    A, D, DEST, DESTS, FIRST, KIDS, NAMES, NEXT, OUTLINES, S, TITLE,
};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, ObjRef, Object, ObjectIdentifier};

use crate::backend::OutlineNode;
use crate::error::{AppError, AppResult};

use super::encoding::decode_pdf_text_string;

pub(super) fn extract_outline_nodes(pdf: &Pdf) -> AppResult<Vec<OutlineNode>> {
    let xref = pdf.xref();
    let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else {
        return Err(AppError::unsupported("failed to resolve pdf catalog"));
    };
    let Some(outlines_ref) = root.get_ref(OUTLINES) else {
        return Ok(Vec::new());
    };
    let Some(outlines) = xref.get::<Dict<'_>>(outlines_ref.into()) else {
        return Ok(Vec::new());
    };
    let Some(first_ref) = outlines.get_ref(FIRST) else {
        return Ok(Vec::new());
    };

    let page_index = pdf
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(page, pdf_page)| pdf_page.raw().obj_id().map(|id| (id, page)))
        .collect::<HashMap<_, _>>();
    let named_destinations = build_named_destination_index(root);
    let mut visited = HashSet::new();

    Ok(read_outline_siblings(
        xref,
        first_ref.into(),
        &page_index,
        &named_destinations,
        &mut visited,
    ))
}

fn read_outline_siblings<'a>(
    xref: &'a hayro::hayro_syntax::xref::XRef,
    start: ObjectIdentifier,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();
    let mut current = Some(start);

    while let Some(id) = current {
        if !visited.insert(id) {
            break;
        }

        let Some(item) = xref.get::<Dict<'_>>(id) else {
            break;
        };
        let next = item.get_ref(NEXT).map(Into::into);
        let mut children = item
            .get_ref(FIRST)
            .map(|first| {
                read_outline_siblings(xref, first.into(), page_index, named_destinations, visited)
            })
            .unwrap_or_default();

        if let Some(page) = resolve_outline_page(&item, xref, page_index, named_destinations) {
            nodes.push(OutlineNode {
                title: outline_title(&item),
                page,
                children,
            });
        } else {
            nodes.append(&mut children);
        }

        current = next;
    }

    nodes
}

fn resolve_outline_page<'a>(
    item: &Dict<'a>,
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
) -> Option<usize> {
    if let Some(dest) = item.get_raw::<Object<'_>>(DEST) {
        return resolve_destination(
            dest,
            xref,
            page_index,
            named_destinations,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
    }

    let action = item.get::<Dict<'_>>(A)?;
    let action_kind = action.get::<Name<'_>>(S)?;
    if action_kind.as_str() != "GoTo" {
        return None;
    }

    let dest = action.get_raw::<Object<'_>>(D)?;
    resolve_destination(
        dest,
        xref,
        page_index,
        named_destinations,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}

fn resolve_destination<'a>(
    value: MaybeRef<Object<'a>>,
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
    visited_names: &mut HashSet<Vec<u8>>,
) -> Option<usize> {
    let object = match value {
        MaybeRef::Ref(obj_ref) => {
            let id: ObjectIdentifier = obj_ref.into();
            if !visited.insert(id) {
                return None;
            }
            xref.get::<Object<'_>>(id)?
        }
        MaybeRef::NotRef(object) => object,
    };

    match object {
        Object::Array(array) => resolve_destination_array(&array, page_index),
        Object::Dict(dict) => dict.get_raw::<Object<'_>>(D).and_then(|dest| {
            resolve_destination(
                dest,
                xref,
                page_index,
                named_destinations,
                visited,
                visited_names,
            )
        }),
        Object::Name(name) => resolve_named_destination(
            name.as_ref(),
            xref,
            page_index,
            named_destinations,
            visited,
            visited_names,
        ),
        Object::String(string) => resolve_named_destination(
            string.as_bytes(),
            xref,
            page_index,
            named_destinations,
            visited,
            visited_names,
        ),
        Object::Null(_) | Object::Boolean(_) | Object::Number(_) | Object::Stream(_) => None,
    }
}

fn resolve_destination_array(
    array: &Array<'_>,
    page_index: &HashMap<ObjectIdentifier, usize>,
) -> Option<usize> {
    match array.raw_iter().next()? {
        MaybeRef::Ref(page_ref) => page_index.get(&page_ref.into()).copied(),
        MaybeRef::NotRef(Object::Dict(page_dict)) => page_dict
            .obj_id()
            .and_then(|id| page_index.get(&id).copied()),
        MaybeRef::NotRef(_) => None,
    }
}

fn outline_title(item: &Dict<'_>) -> String {
    let Some(title) = item.get::<hayro::hayro_syntax::object::String<'_>>(TITLE) else {
        return "(untitled)".to_string();
    };

    let decoded = decode_pdf_text_string(title.as_bytes()).trim().to_string();
    if decoded.is_empty() {
        "(untitled)".to_string()
    } else {
        decoded
    }
}

fn build_named_destination_index<'a>(root: Dict<'a>) -> HashMap<Vec<u8>, NamedDestination<'a>> {
    let mut destinations = HashMap::new();
    let mut visited = HashSet::new();

    if let Some(dests) = root.get::<Dict<'_>>(DESTS) {
        collect_destination_dict(dests, &mut destinations);
    }

    if let Some(names_root) = root.get::<Dict<'_>>(NAMES)
        && let Some(dests_tree) = names_root.get::<Dict<'_>>(DESTS)
    {
        collect_name_tree_destinations(dests_tree, &mut destinations, &mut visited);
    }
    destinations
}

fn collect_destination_dict<'a>(
    dict: Dict<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
) {
    for (key, value) in dict.entries() {
        destinations.insert(
            key.as_ref().to_vec(),
            NamedDestination::from_maybe_ref(value),
        );
    }
}

fn collect_name_tree_destinations<'a>(
    dict: Dict<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
) {
    if let Some(id) = dict.obj_id()
        && !visited.insert(id)
    {
        return;
    }

    if let Some(names) = dict.get::<Array<'_>>(NAMES) {
        collect_name_tree_pairs(names, destinations);
    }

    if let Some(kids) = dict.get::<Array<'_>>(KIDS) {
        for child in kids.iter::<Dict<'_>>() {
            collect_name_tree_destinations(child, destinations, visited);
        }
    }
}

fn collect_name_tree_pairs<'a>(
    array: Array<'a>,
    destinations: &mut HashMap<Vec<u8>, NamedDestination<'a>>,
) {
    let mut iter = array.raw_iter();
    while let Some(name) = iter.next() {
        let Some(dest) = iter.next() else {
            break;
        };

        if let Some(key) = destination_name_key(name) {
            destinations.insert(key, NamedDestination::from_maybe_ref(dest));
        }
    }
}

fn destination_name_key(value: MaybeRef<Object<'_>>) -> Option<Vec<u8>> {
    match value {
        MaybeRef::Ref(_) => None,
        MaybeRef::NotRef(Object::Name(name)) => Some(name.as_ref().to_vec()),
        MaybeRef::NotRef(Object::String(string)) => Some(string.as_bytes().to_vec()),
        MaybeRef::NotRef(_) => None,
    }
}

fn resolve_named_destination<'a>(
    name: &[u8],
    xref: &'a hayro::hayro_syntax::xref::XRef,
    page_index: &HashMap<ObjectIdentifier, usize>,
    named_destinations: &HashMap<Vec<u8>, NamedDestination<'a>>,
    visited: &mut HashSet<ObjectIdentifier>,
    visited_names: &mut HashSet<Vec<u8>>,
) -> Option<usize> {
    if !visited_names.insert(name.to_vec()) {
        return None;
    }
    let dest = named_destinations.get(name)?.to_maybe_ref();
    resolve_destination(
        dest,
        xref,
        page_index,
        named_destinations,
        visited,
        visited_names,
    )
}

#[derive(Debug, Clone)]
enum NamedDestination<'a> {
    Direct(Object<'a>),
    Ref(ObjRef),
}

impl<'a> NamedDestination<'a> {
    fn from_maybe_ref(value: MaybeRef<Object<'a>>) -> Self {
        match value {
            MaybeRef::Ref(obj_ref) => Self::Ref(obj_ref),
            MaybeRef::NotRef(object) => Self::Direct(object),
        }
    }

    fn to_maybe_ref(&self) -> MaybeRef<Object<'a>> {
        match self {
            Self::Direct(object) => MaybeRef::NotRef(object.clone()),
            Self::Ref(obj_ref) => MaybeRef::Ref(*obj_ref),
        }
    }
}
