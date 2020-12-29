/* Copyright 2020 Matt Spraggs
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::cell::RefCell;
use std::rc::Rc;

use crate::chunk::ChunkStore;
use crate::core;
use crate::memory::{Gc, Heap, Root};
use crate::object::{self, ObjClass, ObjStringStore};
use crate::vm;

include!(concat!(env!("OUT_DIR"), "/core.yl.rs"));

#[derive(Clone)]
pub struct CoreClassStore {
    root_obj_base_metaclass: Root<ObjClass>,
    root_obj_iter_class: Root<ObjClass>,
    root_obj_map_iter_class: Root<ObjClass>,
    root_obj_filter_iter_class: Root<ObjClass>,
    root_obj_vec_class: Root<ObjClass>,
    root_obj_vec_iter_class: Root<ObjClass>,
    root_obj_range_class: Root<ObjClass>,
    root_obj_range_iter_class: Root<ObjClass>,
    root_obj_string_iter_class: Root<ObjClass>,
}

impl CoreClassStore {
    pub(crate) fn new(
        heap: &mut Heap,
        string_store: &mut ObjStringStore,
        root_obj_base_metaclass: Root<ObjClass>,
    ) -> Self {
        let empty = string_store.new_gc_obj_string(heap, "");
        let methods = object::new_obj_string_value_map();
        let root_obj_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_map_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_filter_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_vec_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_vec_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_range_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_range_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        let root_obj_string_iter_class = object::new_root_obj_class(
            heap,
            empty,
            root_obj_base_metaclass.as_gc(),
            methods.clone(),
        );
        CoreClassStore {
            root_obj_base_metaclass,
            root_obj_iter_class,
            root_obj_map_iter_class,
            root_obj_filter_iter_class,
            root_obj_vec_class,
            root_obj_vec_iter_class,
            root_obj_range_class,
            root_obj_range_iter_class,
            root_obj_string_iter_class,
        }
    }

    pub(crate) fn get_obj_base_metaclass(&self) -> Gc<ObjClass> {
        self.root_obj_base_metaclass.as_gc()
    }

    pub(crate) fn get_obj_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_iter_class.as_gc()
    }

    pub(crate) fn get_obj_map_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_map_iter_class.as_gc()
    }

    pub(crate) fn get_obj_filter_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_filter_iter_class.as_gc()
    }

    pub(crate) fn get_obj_vec_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_class.as_gc()
    }

    pub(crate) fn get_obj_vec_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_vec_iter_class.as_gc()
    }

    pub(crate) fn get_obj_range_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_class.as_gc()
    }

    pub(crate) fn get_obj_range_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_range_iter_class.as_gc()
    }

    pub(crate) fn get_obj_string_iter_class(&self) -> Gc<ObjClass> {
        self.root_obj_string_iter_class.as_gc()
    }
}

pub fn new_empty_class_store(
    heap: &mut Heap,
    string_store: &mut ObjStringStore,
    root_obj_base_metaclass: Root<ObjClass>,
) -> Box<CoreClassStore> {
    Box::new(CoreClassStore::new(
        heap,
        string_store,
        root_obj_base_metaclass,
    ))
}

pub(crate) fn new_class_store(
    heap: Rc<RefCell<Heap>>,
    string_store: Rc<RefCell<ObjStringStore>>,
    chunk_store: Rc<RefCell<ChunkStore>>,
    root_obj_base_metaclass: Root<ObjClass>,
) -> Box<CoreClassStore> {
    let mut vm = vm::new_root_vm(
        heap.clone(),
        string_store.clone(),
        chunk_store,
        root_obj_base_metaclass.clone(),
    );
    let source = String::from(CORE_SOURCE);
    let result = vm::interpret(&mut vm, source);
    match result {
        Ok(_) => {}
        Err(error) => eprint!("{}", error),
    }
    let root_obj_iter_class = vm
        .get_global("Iter")
        .unwrap()
        .try_as_obj_class()
        .expect("Expected ObjClass.")
        .as_root();
    let root_obj_map_iter_class = vm
        .get_global("MapIter")
        .unwrap()
        .try_as_obj_class()
        .expect("Expected ObjClass.")
        .as_root();
    let root_obj_filter_iter_class = vm
        .get_global("FilterIter")
        .unwrap()
        .try_as_obj_class()
        .expect("Expected ObjClass.")
        .as_root();
    let borrowed_heap = &mut heap.borrow_mut();
    let root_obj_vec_class = core::new_root_obj_vec_class(
        borrowed_heap,
        &mut string_store.borrow_mut(),
        root_obj_base_metaclass.as_gc(),
        root_obj_iter_class.as_gc(),
    );
    let root_obj_vec_iter_class = core::new_root_obj_vec_iter_class(
        borrowed_heap,
        &mut string_store.borrow_mut(),
        root_obj_base_metaclass.as_gc(),
    );
    let root_obj_range_class = core::new_root_obj_range_class(
        borrowed_heap,
        &mut string_store.borrow_mut(),
        root_obj_base_metaclass.as_gc(),
        root_obj_iter_class.as_gc(),
    );
    let root_obj_range_iter_class = core::new_root_obj_range_iter_class(
        borrowed_heap,
        &mut string_store.borrow_mut(),
        root_obj_base_metaclass.as_gc(),
    );
    let root_obj_string_iter_class = core::new_root_obj_string_iter_class(
        borrowed_heap,
        &mut string_store.borrow_mut(),
        root_obj_base_metaclass.as_gc(),
    );
    Box::new(CoreClassStore {
        root_obj_base_metaclass,
        root_obj_iter_class,
        root_obj_map_iter_class,
        root_obj_filter_iter_class,
        root_obj_vec_class,
        root_obj_vec_iter_class,
        root_obj_range_class,
        root_obj_range_iter_class,
        root_obj_string_iter_class,
    })
}
