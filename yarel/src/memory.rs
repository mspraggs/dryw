/* Copyright 2020-2021 Matt Spraggs
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

// The code below is in part inspired by the mark-and-sweep GC implemented here:
// https://github.com/Darksecond/lox

use std::any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomPinned;
use std::mem;
use std::ops::Deref;
use std::pin::Pin;
use std::ptr::{self, NonNull};

use crate::common;
use crate::unsafe_ref_cell::UnsafeRefCell;

#[derive(Copy, Clone, PartialEq)]
enum Colour {
    Black,
    Grey,
    White,
}

pub trait GcManaged {
    fn mark(&self);

    fn blacken(&self);
}

pub(crate) type GcBoxPtr<T> = NonNull<GcBox<T>>;

pub(crate) struct GcBox<T: GcManaged + ?Sized> {
    colour: Cell<Colour>,
    num_roots: Cell<usize>,
    _pin: PhantomPinned,
    pub(crate) data: T,
}

impl<T: 'static + GcManaged + ?Sized> GcBox<T> {
    fn unmark(&self) {
        self.colour.set(Colour::White);
    }

    fn mark(&self) {
        if self.colour.replace(Colour::Grey) == Colour::Grey {
            return;
        }
        if cfg!(feature = "debug_trace_gc") {
            println!("{:?} mark", self as *const _);
        }
        self.data.mark();
    }

    fn blacken(&self) {
        if self.colour.replace(Colour::Black) == Colour::Black {
            return;
        }
        if cfg!(feature = "debug_trace_gc") {
            println!("{:?} blacken", self as *const _);
        }
        self.data.blacken();
    }

    fn inc_num_roots(&self) {
        self.num_roots.replace(self.num_roots.get() + 1);
    }

    fn dec_num_roots(&self) {
        self.num_roots.replace(self.num_roots.get() - 1);
    }
}

pub struct Root<T: 'static + GcManaged + ?Sized> {
    ptr: Option<GcBoxPtr<T>>,
}

impl<T: GcManaged> Root<T> {
    pub fn null() -> Self {
        Root { ptr: None }
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_none()
    }

    pub fn as_gc(&self) -> Gc<T> {
        Gc { ptr: self.ptr }
    }
}

impl<T: 'static + GcManaged + ?Sized> Root<T> {
    fn inc_num_roots(&self) {
        self.gc_box().expect("Expected GcBox.").inc_num_roots();
    }

    fn dec_num_roots(&self) {
        self.gc_box().expect("Expected GcBox.").dec_num_roots();
    }
}

impl<T: GcManaged + ?Sized> Root<T> {
    fn gc_box(&self) -> Option<&GcBox<T>> {
        unsafe { self.ptr.as_ref().map(|p| p.as_ref()) }
    }
}

impl<T: 'static + GcManaged + ?Sized> GcManaged for Root<T> {
    fn mark(&self) {
        match self.gc_box() {
            Some(p) => p.mark(),
            None => {}
        }
    }

    fn blacken(&self) {
        match self.gc_box() {
            Some(p) => p.blacken(),
            None => {}
        }
    }
}

impl<T: 'static + GcManaged + ?Sized> Clone for Root<T> {
    fn clone(&self) -> Root<T> {
        let ret = Root { ptr: self.ptr };
        ret.inc_num_roots();
        ret
    }
}

impl<T: 'static + GcManaged> Deref for Root<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().expect("Expected GcBox.").data
    }
}

impl<T: 'static + GcManaged + ?Sized> Drop for Root<T> {
    fn drop(&mut self) {
        if self.ptr.is_none() {
            return;
        }
        self.dec_num_roots();
    }
}

impl<T: GcManaged> From<Gc<T>> for Root<T> {
    fn from(gc: Gc<T>) -> Self {
        let ret = Root { ptr: gc.ptr };
        ret.inc_num_roots();
        ret
    }
}

impl<T: GcManaged> From<GcBoxPtr<T>> for Root<T> {
    fn from(ptr: GcBoxPtr<T>) -> Self {
        let ret = Root { ptr: Some(ptr) };
        ret.inc_num_roots();
        ret
    }
}

pub struct Gc<T: GcManaged + ?Sized> {
    ptr: Option<GcBoxPtr<T>>,
}

impl<T: GcManaged> Gc<T> {
    pub fn as_root(&self) -> Root<T> {
        Root::from(*self)
    }

    pub(crate) fn null() -> Self {
        Gc { ptr: None }
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_none()
    }
}

impl<T: 'static + GcManaged> Gc<T> {
    pub fn as_ptr(&self) -> *const T {
        match self.gc_box() {
            Some(p) => &p.data,
            None => ptr::null(),
        }
    }
}

impl<T: 'static + GcManaged + ?Sized> Gc<T> {
    fn gc_box(&self) -> Option<&GcBox<T>> {
        unsafe { self.ptr.as_ref().map(|p| p.as_ref()) }
    }
}

impl<T: 'static + GcManaged + ?Sized> GcManaged for Gc<T> {
    fn mark(&self) {
        match self.gc_box() {
            Some(p) => p.mark(),
            None => {}
        }
    }

    fn blacken(&self) {
        match self.gc_box() {
            Some(p) => p.blacken(),
            None => {}
        }
    }
}

impl<T: GcManaged> Copy for Gc<T> {}

impl<T: GcManaged> Clone for Gc<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static + GcManaged> Deref for Gc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.gc_box().expect("Expected GcBox.").data
    }
}

impl<T: GcManaged> PartialEq for Gc<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self.ptr, other.ptr) {
            (Some(p0), Some(p1)) => {p0.as_ptr() == p1.as_ptr()}
            _ => false,
        }
    }
}

#[derive(Default)]
pub struct Heap {
    collection_threshold: usize,
    bytes_allocated: usize,
    objects: Vec<Pin<Box<GcBox<dyn GcManaged>>>>,
}

impl Heap {
    pub(crate) fn new() -> Self {
        Heap {
            collection_threshold: common::HEAP_INIT_BYTES_MAX,
            bytes_allocated: 0,
            objects: Vec::new(),
        }
    }

    pub(crate) fn allocate<T: 'static + GcManaged>(
        &mut self,
        static_roots: &[&dyn GcManaged],
        data: T,
    ) -> Gc<T> {
        Gc {
            ptr: Some(self.allocate_bare(static_roots, data)),
        }
    }
    pub(crate) fn allocate_root<T: 'static + GcManaged>(
        &mut self,
        static_roots: &[&dyn GcManaged],
        data: T,
    ) -> Root<T> {
        self.allocate(static_roots, data).as_root()
    }

    pub(crate) fn allocate_bare<T: 'static + GcManaged>(
        &mut self,
        static_roots: &[&dyn GcManaged],
        data: T,
    ) -> GcBoxPtr<T> {
        if cfg!(any(debug_assertions, feature = "debug_stress_gc")) {
            self.collect(static_roots);
        } else {
            self.collect_if_required(static_roots);
        }
        let mut boxed = Box::pin(GcBox {
            colour: Cell::new(Colour::White),
            num_roots: Cell::new(0),
            _pin: PhantomPinned,
            data,
        });

        let gc_box_ptr = unsafe { GcBoxPtr::new_unchecked(boxed.as_mut().get_unchecked_mut()) };

        self.objects.push(boxed);
        let size = mem::size_of::<T>();

        self.bytes_allocated += size;

        if cfg!(feature = "debug_trace_gc") {
            let new_ptr = self.objects.last().unwrap();
            println!(
                "{:?} allocate {} for {:?}",
                new_ptr.as_ref().get_ref() as *const _,
                size,
                any::type_name::<T>(),
            )
        }

        gc_box_ptr
    }

    fn collect(&mut self, static_roots: &[&dyn GcManaged]) {
        if cfg!(feature = "debug_trace_gc") {
            println!("-- gc begin")
        }

        self.mark_roots(static_roots);
        self.trace_references(static_roots);
        let bytes_freed = self.sweep();

        let prev_bytes_allocated = self.bytes_allocated;
        self.bytes_allocated -= bytes_freed;
        self.collection_threshold = self.bytes_allocated * common::HEAP_GROWTH_FACTOR;

        if cfg!(feature = "debug_trace_gc") {
            println!("-- gc end (freed {} bytes)", bytes_freed);
            println!(
                "   collected {} bytes (from {} to {}) next at {}",
                bytes_freed, prev_bytes_allocated, self.bytes_allocated, self.collection_threshold,
            )
        }
    }

    fn collect_if_required(&mut self, static_roots: &[&dyn GcManaged]) {
        if self.bytes_allocated >= self.collection_threshold {
            self.collect(static_roots);
        }
    }

    fn mark_roots(&mut self, static_roots: &[&dyn GcManaged]) {
        self.objects.iter_mut().for_each(|obj| obj.unmark());
        static_roots.iter().for_each(|o| o.mark());
        self.objects.iter_mut().for_each(|obj| {
            if obj.num_roots.get() > 0 {
                obj.mark();
            }
        });
    }

    fn trace_references(&mut self, static_roots: &[&dyn GcManaged]) {
        let mut num_greys = self
            .objects
            .iter()
            .filter(|obj| obj.colour.get() == Colour::Grey)
            .count();
        static_roots.iter().for_each(|o| o.blacken());
        #[allow(clippy::suspicious_map)]
        while num_greys > 0 {
            num_greys = self
                .objects
                .iter_mut()
                .filter(|obj| obj.colour.get() == Colour::Grey)
                .map(|obj| obj.blacken())
                .count();
        }
    }

    fn sweep(&mut self) -> usize {
        let bytes_marked: usize = self
            .objects
            .iter()
            .filter(|obj| obj.colour.get() == Colour::White)
            .map(|obj| {
                if cfg!(feature = "debug_trace_gc") {
                    println!("{:?} free", obj.as_ref().get_ref() as *const _);
                }
                mem::size_of_val(&obj.data)
            })
            .sum();

        self.objects.retain(|obj| obj.colour.get() == Colour::Black);

        bytes_marked
    }
}

impl<T: GcManaged> GcManaged for RefCell<T> {
    fn mark(&self) {
        self.borrow().mark();
    }

    fn blacken(&self) {
        self.borrow().blacken();
    }
}

impl<T: GcManaged> GcManaged for UnsafeRefCell<T> {
    fn mark(&self) {
        self.borrow().mark();
    }

    fn blacken(&self) {
        self.borrow().blacken();
    }
}

impl<T: GcManaged> GcManaged for Vec<T> {
    fn mark(&self) {
        for e in self {
            e.mark();
        }
    }

    fn blacken(&self) {
        for e in self {
            e.blacken();
        }
    }
}

impl<K, V: GcManaged, S> GcManaged for HashMap<K, V, S> {
    fn mark(&self) {
        for v in self.values() {
            v.mark();
        }
    }

    fn blacken(&self) {
        for v in self.values() {
            v.blacken();
        }
    }
}

impl<T: GcManaged> GcManaged for &[T] {
    fn mark(&self) {
        for i in 0..self.len() {
            self[i].mark();
        }
    }

    fn blacken(&self) {
        for i in 0..self.len() {
            self[i].blacken();
        }
    }
}
