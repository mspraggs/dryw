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
use std::collections::HashMap;
use std::ops::Deref;
use std::time;

use crate::chunk::OpCode;
use crate::common;
use crate::compiler;
use crate::debug;
use crate::memory::{self, Gc, GcManaged};
use crate::object::{self, NativeFn, ObjClass, ObjClosure, ObjFunction, ObjString, ObjUpvalue};
use crate::value::Value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

#[derive(Debug)]
pub enum VmError {
    AttributeError,
    CompileError(Vec<String>),
    IndexError,
    RuntimeError,
    TypeError,
    ValueError,
}

pub fn interpret(vm: &mut Vm, source: String) -> Result<(), VmError> {
    let compile_result = compiler::compile(vm, source);
    match compile_result {
        Ok(function) => vm.interpret(function),
        Err(errors) => Err(VmError::CompileError(errors)),
    }
}

pub struct CallFrame {
    closure: Gc<RefCell<ObjClosure>>,
    ip: usize,
    slot_base: usize,
}

impl memory::GcManaged for CallFrame {
    fn mark(&self) {
        self.closure.mark();
    }

    fn blacken(&self) {
        self.closure.blacken();
    }
}

pub struct Vm {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    globals: HashMap<String, Value>,
    open_upvalues: Vec<Gc<RefCell<ObjUpvalue>>>,
    ephemeral_roots: Vec<Value>,
    init_string: ObjString,
}

impl Default for Vm {
    fn default() -> Self {
        Vm {
            frames: Vec::with_capacity(FRAMES_MAX),
            stack: Vec::with_capacity(STACK_MAX),
            globals: HashMap::new(),
            open_upvalues: Vec::new(),
            ephemeral_roots: Vec::new(),
            init_string: ObjString::from("init"),
        }
    }
}

fn clock_native(_arg_count: usize, _args: &mut [Value]) -> Value {
    let duration = time::SystemTime::now()
        .duration_since(time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    Value::Number(seconds + nanos)
}

impl Vm {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn interpret(&mut self, function: Gc<ObjFunction>) -> Result<(), VmError> {
        self.define_native("clock", clock_native);
        self.push(Value::ObjFunction(function));
        self.ephemeral_roots.clear();

        let closure = object::new_gc_obj_closure(self, function);
        self.pop()?;
        self.push(Value::ObjClosure(closure));
        self.call_value(Value::ObjClosure(closure), 0)?;
        self.run()
    }

    pub fn mark_roots(&mut self) {
        self.stack.mark();
        self.globals.mark();
        self.frames.mark();
        self.ephemeral_roots.mark();
    }

    pub fn push_ephemeral_root(&mut self, root: Value) {
        self.ephemeral_roots.push(root);
    }

    fn run(&mut self) -> Result<(), VmError> {
        macro_rules! binary_op {
            ($value_type:expr, $op:tt) => {
                {
                    let second_value = self.pop()?;
                    let first_value = self.pop()?;
                    let (first, second) = match (first_value, second_value) {
                        (
                            Value::Number(first),
                            Value::Number(second)
                        ) => (first, second),
                        _ => {
                            self.runtime_error("Binary operands must both be numbers.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    self.push($value_type(first $op second));
                }
            };
        }

        macro_rules! read_byte {
            () => {{
                let ip = self.frame()?.ip;
                let ret = self.frame()?.closure.borrow().function.chunk.code[ip];
                self.frames.last_mut().ok_or(VmError::IndexError)?.ip += 1;
                ret
            }};
        }

        macro_rules! read_short {
            () => {{
                let ret = ((self.frame()?.closure.borrow().function.chunk.code[self.frame()?.ip]
                    as u16)
                    << 8)
                    | self.frame()?.closure.borrow().function.chunk.code[self.frame()?.ip + 1]
                        as u16;
                self.frame_mut()?.ip += 2;
                ret
            }};
        }

        macro_rules! read_constant {
            () => {{
                let index = read_byte!() as usize;
                self.frame()?.closure.borrow().function.chunk.constants[index]
            }};
        }

        macro_rules! read_string {
            () => {
                match read_constant!() {
                    Value::ObjString(s) => s,
                    _ => panic!("Expected variable name."),
                }
            };
        }

        loop {
            if cfg!(feature = "debug_trace") {
                print!("          ");
                for v in self.stack.iter() {
                    print!("[ {} ]", v);
                }
                println!();
                let ip = self.frame()?.ip;
                debug::disassemble_instruction(&self.frame()?.closure.borrow().function.chunk, ip);
            }
            let instruction = OpCode::from(read_byte!());

            match instruction {
                OpCode::Constant => {
                    let constant = read_constant!();
                    self.push(constant);
                }

                OpCode::Nil => {
                    self.push(Value::None);
                }

                OpCode::True => {
                    self.push(Value::Boolean(true));
                }

                OpCode::False => {
                    self.push(Value::Boolean(false));
                }

                OpCode::Pop => {
                    self.pop()?;
                }

                OpCode::GetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame()?.slot_base;
                    let value = self.stack[slot_base + slot];
                    self.push(value);
                }

                OpCode::SetLocal => {
                    let slot = read_byte!() as usize;
                    let slot_base = self.frame()?.slot_base;
                    self.stack[slot_base + slot] = *self.peek(0);
                }

                OpCode::GetGlobal => {
                    let name = read_string!();
                    let value = match self.globals.get(name.deref()) {
                        Some(value) => *value,
                        None => {
                            let msg = format!("Undefined variable '{}'.", *name);
                            self.runtime_error(msg.as_str());
                            return Err(VmError::RuntimeError);
                        }
                    };
                    self.push(value);
                }

                OpCode::DefineGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    self.globals.insert((*name).clone(), value);
                    self.pop()?;
                }

                OpCode::SetGlobal => {
                    let name = read_string!();
                    let value = *self.peek(0);
                    let prev = self.globals.insert((*name).clone(), value);
                    match prev {
                        Some(_) => {}
                        None => {
                            self.globals.remove(name.deref());
                            let msg = format!("Undefined variable '{}'.", *name);
                            self.runtime_error(msg.as_str());
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                OpCode::GetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue =
                        match *self.frame()?.closure.borrow().upvalues[upvalue_index].borrow() {
                            ObjUpvalue::Open(slot) => self.stack[slot],
                            ObjUpvalue::Closed(value) => value,
                        };
                    self.push(upvalue);
                }

                OpCode::SetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let stack_value = *self.peek(0);
                    let closure = self.frame()?.closure;
                    match *closure.borrow_mut().upvalues[upvalue_index].borrow_mut() {
                        ObjUpvalue::Open(slot) => {
                            self.stack[slot] = stack_value;
                        }
                        ObjUpvalue::Closed(ref mut value) => {
                            *value = stack_value;
                        }
                    };
                }

                OpCode::GetProperty => {
                    let instance = match *self.peek(0) {
                        Value::ObjInstance(ptr) => ptr,
                        _ => {
                            self.runtime_error("Only instances have properties.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let name = read_string!();

                    let borrowed_instance = instance.borrow();
                    if let Some(property) = borrowed_instance.fields.get(name.deref()) {
                        self.pop()?;
                        self.push(*property);
                    } else {
                        self.bind_method(borrowed_instance.class, name)?;
                    }
                }

                OpCode::SetProperty => {
                    let instance = match *self.peek(1) {
                        Value::ObjInstance(ptr) => ptr,
                        _ => {
                            self.runtime_error("Only instances have fields.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let name = read_string!();
                    let value = *self.peek(0);
                    instance.borrow_mut().fields.insert((*name).clone(), value);

                    self.pop()?;
                    self.pop()?;
                    self.push(value);
                }

                OpCode::GetSuper => {
                    let name = read_string!();
                    let superclass = match self.pop()? {
                        Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };

                    self.bind_method(superclass, name)?;
                }

                OpCode::Equal => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::Boolean(a == b));
                }

                OpCode::Greater => binary_op!(Value::Boolean, >),

                OpCode::Less => binary_op!(Value::Boolean, <),

                OpCode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::ObjString(a), Value::ObjString(b)) => {
                            let value = Value::ObjString(object::new_gc_obj_string(
                                self,
                                format!("{}{}", *a, *b).as_str(),
                            ));
                            self.stack.push(value)
                        }

                        (Value::Number(a), Value::Number(b)) => {
                            self.push(Value::Number(a + b));
                        }

                        _ => {
                            self.runtime_error(
                                "Binary operands must be two numbers or two strings.",
                            );
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                OpCode::Subtract => binary_op!(Value::Number, -),

                OpCode::Multiply => binary_op!(Value::Number, *),

                OpCode::Divide => binary_op!(Value::Number, /),

                OpCode::Not => {
                    let value = self.pop()?;
                    self.push(Value::Boolean(!value.as_bool()));
                }

                OpCode::Negate => {
                    let value = self.pop()?;
                    match value {
                        Value::Number(underlying) => {
                            self.push(Value::Number(-underlying));
                        }
                        _ => {
                            self.runtime_error("Unary operand must be a number.");
                            return Err(VmError::RuntimeError);
                        }
                    }
                }

                OpCode::Print => {
                    println!("{}", self.pop()?);
                }

                OpCode::Jump => {
                    let offset = read_short!();
                    self.frame_mut()?.ip += offset as usize;
                }

                OpCode::JumpIfFalse => {
                    let offset = read_short!();
                    if !self.peek(0).as_bool() {
                        self.frame_mut()?.ip += offset as usize;
                    }
                }

                OpCode::Loop => {
                    let offset = read_short!();
                    self.frame_mut()?.ip -= offset as usize;
                }

                OpCode::Call => {
                    let arg_count = read_byte!() as usize;
                    self.call_value(*self.peek(arg_count), arg_count)?;
                }

                OpCode::Invoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    self.invoke(method, arg_count)?;
                }

                OpCode::SuperInvoke => {
                    let method = read_string!();
                    let arg_count = read_byte!() as usize;
                    let superclass = match self.pop()? {
                        Value::ObjClass(ptr) => ptr,
                        _ => unreachable!(),
                    };
                    self.invoke_from_class(superclass, method, arg_count)?;
                }

                OpCode::Closure => {
                    let function = match read_constant!() {
                        Value::ObjFunction(underlying) => underlying,
                        _ => panic!("Expected ObjFunction."),
                    };

                    let upvalue_count = function.upvalue_count;

                    let closure = object::new_gc_obj_closure(self, function);
                    self.push(Value::ObjClosure(closure));

                    for i in 0..upvalue_count {
                        let is_local = read_byte!() != 0;
                        let index = read_byte!() as usize;
                        let slot_base = self.frame()?.slot_base;
                        closure.borrow_mut().upvalues[i] = if is_local {
                            self.capture_upvalue(slot_base + index)
                        } else {
                            self.frame()?.closure.borrow().upvalues[index]
                        };
                    }
                }

                OpCode::CloseUpvalue => {
                    self.close_upvalues(self.stack.len() - 1, *self.peek(0));
                    self.pop()?;
                }

                OpCode::Return => {
                    let result = self.pop()?;
                    for i in self.frame()?.slot_base..self.stack.len() {
                        self.close_upvalues(i, self.stack[i])
                    }

                    let prev_stack_size = self.frame()?.slot_base;
                    self.frames.pop();
                    if self.frames.is_empty() {
                        self.pop()?;
                        return Ok(());
                    }

                    self.stack.truncate(prev_stack_size);
                    self.push(result);
                }

                OpCode::Class => {
                    let string = read_string!();
                    let class = object::new_gc_obj_class(self, string);
                    self.push(Value::ObjClass(class));
                }

                OpCode::Inherit => {
                    let superclass_pos = self.stack.len() - 2;
                    let superclass = match self.stack[superclass_pos] {
                        Value::ObjClass(ptr) => ptr,
                        _ => {
                            self.runtime_error("Superclass must be a class.");
                            return Err(VmError::RuntimeError);
                        }
                    };
                    let subclass = match self.peek(0) {
                        Value::ObjClass(ptr) => *ptr,
                        _ => unreachable!(),
                    };
                    for (name, value) in superclass.borrow().methods.iter() {
                        subclass.borrow_mut().methods.insert(name.clone(), *value);
                    }
                    self.pop()?;
                }

                OpCode::Method => {
                    let name = read_string!();
                    self.define_method(name)?;
                }
            }
        }
    }

    fn call_value(&mut self, value: Value, arg_count: usize) -> Result<(), VmError> {
        match value {
            Value::ObjBoundMethod(bound) => {
                *self.peek_mut(arg_count) = bound.borrow().receiver;
                self.call(bound.borrow().method, arg_count)
            }

            Value::ObjClass(class) => {
                // let instance = memory::allocate(self, RefCell::new(ObjInstance::new(class)));
                let instance = object::new_gc_obj_instance(self, class);
                *self.peek_mut(arg_count) = Value::ObjInstance(instance);

                if let Some(Value::ObjClosure(initialiser)) =
                    class.borrow().methods.get(&self.init_string)
                {
                    return self.call(*initialiser, arg_count);
                } else if arg_count != 0 {
                    let msg = format!("Expected 0 arguments but got {}.", arg_count);
                    self.runtime_error(msg.as_str());
                    return Err(VmError::TypeError);
                }

                Ok(())
            }

            Value::ObjClosure(function) => self.call(function, arg_count),

            Value::ObjNative(wrapped) => {
                let function = wrapped.function.ok_or(VmError::ValueError)?;
                let frame_begin = self.stack.len() - arg_count - 1;
                let result = function(arg_count, &mut self.stack[frame_begin..]);
                self.stack.truncate(frame_begin);
                self.push(result);
                Ok(())
            }

            _ => {
                self.runtime_error("Can only call functions and classes.");
                Err(VmError::TypeError)
            }
        }
    }

    fn invoke_from_class(
        &mut self,
        class: Gc<RefCell<ObjClass>>,
        name: Gc<ObjString>,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if let Some(value) = class.borrow().methods.get(name.deref()) {
            return match value {
                Value::ObjClosure(closure) => self.call(*closure, arg_count),
                _ => unreachable!(),
            };
        }
        let msg = format!("Undefined property '{}'.", *name);
        self.runtime_error(msg.as_str());
        Err(VmError::AttributeError)
    }

    fn invoke(&mut self, name: Gc<ObjString>, arg_count: usize) -> Result<(), VmError> {
        let receiver = *self.peek(arg_count);
        match receiver {
            Value::ObjInstance(instance) => {
                if let Some(value) = instance.borrow().fields.get(name.deref()) {
                    *self.peek_mut(arg_count) = *value;
                    return self.call_value(*value, arg_count);
                }

                self.invoke_from_class(instance.borrow().class, name, arg_count)
            }
            _ => {
                self.runtime_error("Only instances have methods.");
                Err(VmError::ValueError)
            }
        }
    }

    fn call(&mut self, closure: Gc<RefCell<ObjClosure>>, arg_count: usize) -> Result<(), VmError> {
        if arg_count as u32 != closure.borrow().function.arity {
            let msg = format!(
                "Expected {} arguments but got {}.",
                closure.borrow().function.arity,
                arg_count
            );
            self.runtime_error(msg.as_str());
            return Err(VmError::TypeError);
        }

        if self.frames.len() == FRAMES_MAX {
            self.runtime_error("Stack overflow.");
            return Err(VmError::IndexError);
        }

        self.frames.push(CallFrame {
            closure,
            ip: 0,
            slot_base: self.stack.len() - arg_count - 1,
        });
        Ok(())
    }

    fn reset_stack(&mut self) {
        self.stack.clear();
        self.frames.clear();
    }

    fn runtime_error(&mut self, message: &str) {
        eprintln!("{}", message);

        for frame in self.frames.iter().rev() {
            let function = frame.closure.borrow().function;

            let instruction = frame.ip - 1;
            eprint!("[line {}] in ", function.chunk.lines[instruction]);
            if function.name.is_empty() {
                eprintln!("script");
            } else {
                eprintln!("{}()", *function.name);
            }
        }

        self.reset_stack();
    }

    fn define_native(&mut self, name: &str, function: NativeFn) {
        let value = Value::ObjNative(object::new_gc_obj_native(self, function));
        self.push(value);
        let value = *self.peek(0);
        self.globals.insert(String::from(name), value);
        self.pop().unwrap_or(Value::None);
    }

    fn define_method(&mut self, name: Gc<ObjString>) -> Result<(), VmError> {
        let method = *self.peek(0);
        let class = match *self.peek(1) {
            Value::ObjClass(ptr) => ptr,
            _ => unreachable!(),
        };
        class.borrow_mut().methods.insert((*name).clone(), method);
        self.pop().unwrap_or(Value::None);

        Ok(())
    }

    fn bind_method(
        &mut self,
        class: Gc<RefCell<ObjClass>>,
        name: Gc<ObjString>,
    ) -> Result<(), VmError> {
        let borrowed_class = class.borrow();
        let method = match borrowed_class.methods.get(name.deref()) {
            Some(Value::ObjClosure(ptr)) => *ptr,
            None => {
                let msg = format!("Undefined property '{}'.", *name);
                self.runtime_error(msg.as_str());
                return Err(VmError::AttributeError);
            }
            _ => unreachable!(),
        };

        let instance = *self.peek(0);
        let bound = object::new_gc_obj_bound_method(self, instance, method);
        self.pop()?;
        self.push(Value::ObjBoundMethod(bound));

        Ok(())
    }

    fn capture_upvalue(&mut self, location: usize) -> Gc<RefCell<ObjUpvalue>> {
        let result = self
            .open_upvalues
            .iter()
            .find(|&u| u.borrow().is_open_with_index(location));

        let upvalue = if let Some(upvalue) = result {
            *upvalue
        } else {
            object::new_gc_obj_upvalue(self, location)
        };

        self.open_upvalues.push(upvalue);
        upvalue
    }

    fn close_upvalues(&mut self, last: usize, value: Value) {
        for upvalue in self.open_upvalues.iter() {
            if upvalue.borrow().is_open_with_index(last) {
                upvalue.borrow_mut().close(value);
            }
        }

        self.open_upvalues.retain(|u| u.borrow().is_open());
    }

    fn frame(&self) -> Result<&CallFrame, VmError> {
        self.frames.last().ok_or(VmError::IndexError)
    }

    fn frame_mut(&mut self) -> Result<&mut CallFrame, VmError> {
        self.frames.last_mut().ok_or(VmError::IndexError)
    }

    fn peek(&self, depth: usize) -> &Value {
        let stack_len = self.stack.len();
        &self.stack[stack_len - depth - 1]
    }

    fn peek_mut(&mut self, depth: usize) -> &mut Value {
        let stack_len = self.stack.len();
        &mut self.stack[stack_len - depth - 1]
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Result<Value, VmError> {
        self.stack.pop().ok_or(VmError::IndexError)
    }
}
