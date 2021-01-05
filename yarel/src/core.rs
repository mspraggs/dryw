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

use std::char;
use std::time;

use crate::common;
use crate::error::{Error, ErrorKind};
use crate::memory::{Gc, GcBoxPtr, Root};
use crate::object::{self, NativeFn, ObjClass, ObjNative, ObjString, ObjStringValueMap};
use crate::utils;
use crate::value::Value;
use crate::vm::Vm;

fn check_num_args(args: &[Value], expected: usize) -> Result<(), Error> {
    if args.len() != expected + 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected {} parameter{} but found {}.",
            expected,
            if expected == 1 { "" } else { "s" },
            args.len() - 1
        );
    }
    Ok(())
}

fn build_methods(
    vm: &mut Vm,
    definitions: &[(&str, NativeFn)],
    extra_methods: Option<ObjStringValueMap>,
) -> (ObjStringValueMap, Vec<Root<ObjNative>>) {
    let mut roots = Vec::new();
    let mut methods = extra_methods.unwrap_or(object::new_obj_string_value_map());

    for (name, native) in definitions {
        let name = vm.new_gc_obj_string(name);
        let obj_native = object::new_root_obj_native(vm, *native);
        roots.push(obj_native.clone());
        methods.insert(name, Value::ObjNative(obj_native.as_gc()));
    }

    (methods, roots)
}

/// Global functions

pub(crate) fn clock(_vm: &mut Vm, _args: &[Value]) -> Result<Value, Error> {
    let duration = match time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => {
            return error!(ErrorKind::RuntimeError, "Error calling native function.");
        }
    };
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    Ok(Value::Number(seconds + nanos))
}

pub(crate) fn print(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    if args.len() != 2 {
        return error!(ErrorKind::RuntimeError, "Expected one argument to 'print'.");
    }
    println!("{}", args[1]);
    Ok(Value::None)
}

pub(crate) fn type_(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    Ok(Value::ObjClass(match args[1] {
        Value::Boolean(_) => vm.class_store.get_boolean_class(),
        Value::Number(_) => vm.class_store.get_number_class(),
        Value::ObjString(string) => string.class,
        Value::ObjStringIter(_) => vm.class_store.get_obj_string_iter_class(),
        Value::ObjFunction(_) => unreachable!(),
        Value::ObjNative(_) => vm.class_store.get_obj_native_class(),
        Value::ObjClosure(_) => vm.class_store.get_obj_closure_class(),
        Value::ObjClass(class) => class.metaclass,
        Value::ObjInstance(instance) => instance.borrow().class,
        Value::ObjBoundMethod(_) => vm.class_store.get_obj_closure_method_class(),
        Value::ObjBoundNative(_) => vm.class_store.get_obj_native_method_class(),
        Value::ObjVec(vec) => vec.borrow().class,
        Value::ObjVecIter(iter) => iter.borrow().class,
        Value::ObjRange(range) => range.class,
        Value::ObjRangeIter(iter) => iter.borrow().class,
        Value::None => vm.class_store.get_nil_class(),
        Value::Sentinel => vm.class_store.get_sentinel_class(),
    }))
}

pub(crate) fn no_init(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    let class = type_(vm, &[Value::None, args[0]])?;
    error!(ErrorKind::RuntimeError, "Construction of type {} is unsupported.", class)
}

pub(crate) fn build_unsupported_methods(vm: &mut Vm) -> (object::ObjStringValueMap, Vec<Root<ObjNative>>) {
    let method_map = &[
        ("__init__", no_init as NativeFn),
    ];
    build_methods(vm, method_map, None)
}

pub(crate) fn sentinel(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    if args.len() != 1 {
        return error!(
            ErrorKind::RuntimeError,
            "Expected no arguments to 'sentinel'."
        );
    }
    Ok(Value::Sentinel)
}

/// String implementation

pub(crate) unsafe fn bind_gc_obj_string_class(
    vm: &mut Vm,
    class: &mut GcBoxPtr<ObjClass>,
    metaclass: &mut GcBoxPtr<ObjClass>,
) {
    let static_method_map = [
        ("from_ascii", string_from_ascii as NativeFn),
        ("from_utf8", string_from_utf8 as NativeFn),
        ("from_code_points", string_from_code_points as NativeFn),
    ];
    let (static_methods, _native_roots) = build_methods(vm, &static_method_map, None);

    metaclass.as_mut().data.methods = static_methods;

    let method_map = [
        ("__init__", string_init as NativeFn),
        ("__getitem__", string_get_item as NativeFn),
        ("__iter__", string_iter as NativeFn),
        ("len", string_len as NativeFn),
        ("count_chars", string_count_chars as NativeFn),
        ("char_byte_index", string_char_byte_index as NativeFn),
        ("find", string_find as NativeFn),
        ("replace", string_replace as NativeFn),
        ("split", string_split as NativeFn),
        ("starts_with", string_starts_with as NativeFn),
        ("ends_with", string_ends_with as NativeFn),
        ("as_num", string_as_num as NativeFn),
        ("to_bytes", string_to_bytes as NativeFn),
        ("to_code_points", string_to_code_points as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, None);

    class.as_mut().data.methods = methods;
}

fn string_from_ascii(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec_arg = args[1].try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", args[1]),
        )
    })?;

    let mut bytes = Vec::with_capacity(vec_arg.borrow().elements.len() * 2);

    for value in vec_arg.borrow().elements.iter() {
        let num = value.try_as_number().ok_or_else(|| {
            Error::with_message(
                ErrorKind::TypeError,
                &format!("Expected a number but found '{}'.", value),
            )
        })?;
        if num < 0.0 || num > 255.0 || num.trunc() != num {
            return error!(
                ErrorKind::ValueError,
                "Expected a positive integer less than 256 but found '{}'.", num
            );
        }
        if num > 127.0 {
            bytes.push(195_u8);
            bytes.push((num as u8) & 0b1011_1111);
        } else {
            bytes.push(num as u8);
        }
    }

    let string = vm.new_gc_obj_string(&String::from_utf8(bytes).map_err(|_| {
        Error::with_message(
            ErrorKind::ValueError,
            &format!("Unable to create a string from byte sequence."),
        )
    })?);

    Ok(Value::ObjString(string))
}

fn string_from_utf8(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec_arg = args[1].try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", args[1]),
        )
    })?;

    let bytes: Result<Vec<u8>, Error> = vec_arg
        .borrow()
        .elements
        .iter()
        .map(|v| {
            let num = v.try_as_number().ok_or_else(|| {
                Error::with_message(
                    ErrorKind::TypeError,
                    &format!("Expected a number but found '{}'.", v),
                )
            })?;
            if num < 0.0 || num > 255.0 || num.trunc() != num {
                error!(
                    ErrorKind::ValueError,
                    "Expected a positive integer less than 256 but found '{}'.", num
                )
            } else {
                Ok(num as u8)
            }
        })
        .collect();

    let string = vm.new_gc_obj_string(&String::from_utf8(bytes?).map_err(|e| {
        let index = e.utf8_error().valid_up_to();
        let byte = e.into_bytes()[index];
        Error::with_message(
            ErrorKind::ValueError,
            &format!(
                "Invalid Unicode encountered at byte {} with index {}.",
                byte, index,
            ),
        )
    })?);

    Ok(Value::ObjString(string))
}

fn string_from_code_points(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec_arg = args[1].try_as_obj_vec().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            &format!("Expected a Vec instance but found '{}'.", args[1]),
        )
    })?;

    let string: Result<String, Error> = vec_arg
        .borrow()
        .elements
        .iter()
        .map(|v| {
            let num = v.try_as_number().ok_or_else(|| {
                Error::with_message(
                    ErrorKind::TypeError,
                    &format!("Expected a number but found '{}'.", v),
                )
            })?;
            if num < 0.0 || num > u32::MAX as f64 || num.trunc() != num {
                error!(
                    ErrorKind::ValueError,
                    "Expected a positive integer less than {} but found '{}'.",
                    u32::MAX,
                    num
                )
            } else {
                char::from_u32(num as u32).ok_or_else(|| {
                    Error::with_message(
                        ErrorKind::ValueError,
                        &format!(
                            "Expected a valid Unicode code point but found '{}'.",
                            num as u32
                        ),
                    )
                })
            }
        })
        .collect();

    let string = vm.new_gc_obj_string(&string?);

    Ok(Value::ObjString(string))
}

fn string_init(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    Ok(Value::ObjString(
        vm.new_gc_obj_string(format!("{}", args[1]).as_str()),
    ))
}

fn string_get_item(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let string_len = string.len() as isize;

    let (begin, end) = match args[1] {
        Value::Number(_) => {
            let begin = get_bounded_index(args[1], string_len, "String index out of bounds.")?;
            check_char_boundary(string, begin, "string index")?;
            let mut end = begin + 1;
            while end <= string.len() && !string.as_str().is_char_boundary(end) {
                end += 1;
            }
            (begin, end)
        }
        Value::ObjRange(r) => {
            let (begin, end) = r.get_bounded_range(string_len, "String")?;
            check_char_boundary(string, begin, "string slice start")?;
            check_char_boundary(string, end, "string slice end")?;
            (begin, end)
        }
        _ => return error!(ErrorKind::TypeError, "Expected an integer or range."),
    };

    let new_string = vm.new_gc_obj_string(&string.as_str()[begin..end]);

    Ok(Value::ObjString(new_string))
}

fn string_iter(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_string_iter(
        vm,
        vm.class_store.get_obj_string_iter_class(),
        args[0]
            .try_as_obj_string()
            .expect("Expected ObjString instance."),
    );
    Ok(Value::ObjStringIter(iter.as_gc()))
}

fn string_len(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.len() as f64))
}

fn string_count_chars(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    Ok(Value::Number(string.chars().count() as f64))
}

fn string_char_byte_index(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let char_index = get_bounded_index(
        args[1],
        string.as_str().chars().count() as isize,
        "String index parameter out of bounds.",
    )?;

    let mut char_count = 0;
    for i in 0..string.len() + 1 {
        if string.as_str().is_char_boundary(i) {
            if char_count == char_index {
                return Ok(Value::Number(i as f64));
            }
            char_count += 1;
        }
    }
    error!(
        ErrorKind::IndexError,
        "Provided character index out of range."
    )
}

fn string_find(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let substring = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if substring.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot find empty string.");
    }
    let string_len = string.len() as isize;
    let start = {
        let i = utils::validate_integer(args[2])?;
        if i < 0 {
            i + string_len
        } else {
            i
        }
    };
    if start < 0 || start >= string_len {
        return error!(ErrorKind::ValueError, "String index out of bounds.");
    }
    let start = start as usize;
    check_char_boundary(string, start, "string index")?;
    for i in start..string.as_str().len() {
        if !string.is_char_boundary(i) || !string.is_char_boundary(i + substring.len()) {
            continue;
        }
        let slice = &string[i..i + substring.len()];
        if i >= start && slice == substring.as_str() {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::None)
}

fn string_replace(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let old = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if old.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot replace empty string.");
    }
    let new = args[2].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[2]),
        )
    })?;
    let new_string = vm.new_gc_obj_string(&string.replace(old.as_str(), new.as_str()));
    Ok(Value::ObjString(new_string))
}

fn string_split(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let delim = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            &format!("Expected a string but found '{}'.", args[1]),
        )
    })?;
    if delim.is_empty() {
        return error!(ErrorKind::ValueError, "Cannot split using an empty string.");
    }
    let splits = object::new_root_obj_vec(vm, vm.class_store.get_obj_vec_class());
    for substr in string.as_str().split(delim.as_str()) {
        let new_str = Value::ObjString(vm.new_gc_obj_string(substr));
        splits.borrow_mut().elements.push(new_str);
    }
    Ok(Value::ObjVec(splits.as_gc()))
}

fn string_starts_with(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let prefix = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", args[1]).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().starts_with(prefix.as_str())))
}

fn string_ends_with(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let prefix = args[1].try_as_obj_string().ok_or_else(|| {
        Error::with_message(
            ErrorKind::TypeError,
            format!("Expected a string but found '{}'.", args[1]).as_str(),
        )
    })?;

    Ok(Value::Boolean(string.as_str().ends_with(prefix.as_str())))
}

fn string_as_num(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");
    let num = string.parse::<f64>().or_else(|_| {
        error!(
            ErrorKind::ValueError,
            "Unable to parse number from '{}'.", args[0]
        )
    })?;

    Ok(Value::Number(num))
}

fn string_to_bytes(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");

    let vec = object::new_gc_obj_vec(vm, vm.class_store.get_obj_vec_class());
    vec.borrow_mut().elements = string
        .as_bytes()
        .iter()
        .map(|&b| Value::Number(b as f64))
        .collect();

    Ok(Value::ObjVec(vec))
}

fn string_to_code_points(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let string = args[0].try_as_obj_string().expect("Expected ObjString.");

    let vec = object::new_gc_obj_vec(vm, vm.class_store.get_obj_vec_class());
    vec.borrow_mut().elements = string
        .chars()
        .map(|c| Value::Number((c as u32) as f64))
        .collect();

    Ok(Value::ObjVec(vec))
}

fn check_char_boundary(string: Gc<ObjString>, pos: usize, desc: &str) -> Result<(), Error> {
    if !string.as_str().is_char_boundary(pos) {
        return error!(
            ErrorKind::IndexError,
            "Provided {} is not on a character boundary.", desc
        );
    }
    Ok(())
}

/// StringIter implementation

fn string_iter_next(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_string_iter()
        .expect("Expected ObjIter instance.");
    let iterable = iter.borrow().iterable;
    let next = {
        let mut borrowed_iter = iter.borrow_mut();
        borrowed_iter.next()
    };
    if let Some((begin, end)) = next {
        let slice = &iterable[begin..end];
        let string = vm.new_gc_obj_string(slice);
        return Ok(Value::ObjString(string));
    }
    Ok(Value::Sentinel)
}

pub fn new_root_obj_string_iter_class(vm: &mut Vm, metaclass: Gc<ObjClass>) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("StringIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("__next__", string_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, methods)
}

/// Vec implemenation

pub fn new_root_obj_vec_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    iter_class: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Vec");
    let method_map = [
        ("__init__", vec_init as NativeFn),
        ("push", vec_push as NativeFn),
        ("pop", vec_pop as NativeFn),
        ("__getitem__", vec_get_item as NativeFn),
        ("__setitem__", vec_set_item as NativeFn),
        ("len", vec_len as NativeFn),
        ("__iter__", vec_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, Some(iter_class.methods.clone()));
    object::new_root_obj_class(vm, class_name, metaclass, methods)
}

fn vec_init(vm: &mut Vm, _args: &[Value]) -> Result<Value, Error> {
    let vec = object::new_root_obj_vec(vm, vm.class_store.get_obj_vec_class());
    Ok(Value::ObjVec(vec.as_gc()))
}

fn vec_push(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    if vec.borrow().elements.len() >= common::VEC_ELEMS_MAX {
        return error!(ErrorKind::RuntimeError, "Vec max capcity reached.");
    }

    vec.borrow_mut().elements.push(args[1]);

    Ok(args[0])
}

fn vec_pop(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements.pop().ok_or_else(|| {
        Error::with_message(
            ErrorKind::RuntimeError,
            "Cannot pop from empty Vec instance.",
        )
    })
}

fn vec_get_item(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 1)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");

    match args[1] {
        Value::Number(_) => {
            let borrowed_vec = vec.borrow();
            let index = get_bounded_index(
                args[1],
                borrowed_vec.elements.len() as isize,
                "Vec index parameter out of bounds",
            )?;
            Ok(borrowed_vec.elements[index])
        }
        Value::ObjRange(r) => {
            let vec_len = vec.borrow().elements.len() as isize;
            let (begin, end) = r.get_bounded_range(vec_len, "Vec")?;
            let new_vec = object::new_gc_obj_vec(vm, vm.class_store.get_obj_vec_class());
            new_vec
                .borrow_mut()
                .elements
                .extend_from_slice(&vec.borrow().elements[begin..end]);
            Ok(Value::ObjVec(new_vec))
        }
        _ => error!(ErrorKind::TypeError, "Expected an integer or range."),
    }
}

fn vec_set_item(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let index = get_bounded_index(
        args[1],
        vec.borrow().elements.len() as isize,
        "Vec index parameter out of bounds",
    )?;
    let mut borrowed_vec = vec.borrow_mut();
    borrowed_vec.elements[index] = args[2];
    Ok(Value::None)
}

fn vec_len(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let vec = args[0].try_as_obj_vec().expect("Expected ObjVec");
    let borrowed_vec = vec.borrow();
    Ok(Value::from(borrowed_vec.elements.len() as f64))
}

fn vec_iter(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_vec_iter(
        vm,
        vm.class_store.get_obj_vec_iter_class(),
        args[0].try_as_obj_vec().expect("Expected ObjVec instance."),
    );
    Ok(Value::ObjVecIter(iter.as_gc()))
}

fn get_bounded_index(value: Value, bound: isize, msg: &str) -> Result<usize, Error> {
    let mut index = utils::validate_integer(value)?;
    if index < 0 {
        index += bound;
    }
    if index < 0 || index >= bound {
        return error!(ErrorKind::IndexError, "{}", msg);
    }

    Ok(index as usize)
}

/// VecIter implementation

pub fn new_root_obj_vec_iter_class(vm: &mut Vm, metaclass: Gc<ObjClass>) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("VecIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("__next__", vec_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, methods)
}

fn vec_iter_next(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_vec_iter()
        .expect("Expected ObjVecIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

/// Range implementation

pub fn new_root_obj_range_class(
    vm: &mut Vm,
    metaclass: Gc<ObjClass>,
    iter_class: Gc<ObjClass>,
) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("Range");
    let method_map = [
        ("__init__", range_init as NativeFn),
        ("__iter__", range_iter as NativeFn),
    ];
    let (methods, _native_roots) = build_methods(vm, &method_map, Some(iter_class.methods.clone()));
    object::new_root_obj_class(vm, class_name, metaclass, methods)
}

fn range_init(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 2)?;

    let mut bounds: [isize; 2] = [0; 2];
    for i in 0..2 {
        bounds[i] = utils::validate_integer(args[i + 1])?;
    }
    let range = object::new_root_obj_range(
        vm,
        vm.class_store.get_obj_range_class(),
        bounds[0],
        bounds[1],
    );
    Ok(Value::ObjRange(range.as_gc()))
}

fn range_iter(vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    check_num_args(args, 0)?;

    let iter = object::new_root_obj_range_iter(
        vm,
        vm.class_store.get_obj_range_iter_class(),
        args[0]
            .try_as_obj_range()
            .expect("Expected ObjRange instance."),
    );
    Ok(Value::ObjRangeIter(iter.as_gc()))
}

/// RangeIter implementation

fn range_iter_next(_vm: &mut Vm, args: &[Value]) -> Result<Value, Error> {
    assert!(args.len() == 1);
    let iter = args[0]
        .try_as_obj_range_iter()
        .expect("Expected ObjIter instance.");
    let mut borrowed_iter = iter.borrow_mut();
    Ok(borrowed_iter.next())
}

pub fn new_root_obj_range_iter_class(vm: &mut Vm, metaclass: Gc<ObjClass>) -> Root<ObjClass> {
    let class_name = vm.new_gc_obj_string("RangeIter");
    let (methods, _native_roots) =
        build_methods(vm, &[("__next__", range_iter_next as NativeFn)], None);
    object::new_root_obj_class(vm, class_name, metaclass, methods)
}
