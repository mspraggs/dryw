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
use std::ops::DerefMut;
use std::time;

use crate::chunk;
use crate::common;
use crate::compiler;
use crate::debug;
use crate::memory;
use crate::object;
use crate::value;

const FRAMES_MAX: usize = 64;
const STACK_MAX: usize = common::LOCALS_MAX * FRAMES_MAX;

pub enum InterpretResult {
    Ok,
    CompileError,
    RuntimeError,
}

pub fn interpret(vm: &mut Vm, source: String) -> InterpretResult {
    let compile_result = compiler::compile(source);
    match compile_result {
        Some(function) => vm.interpret(function),
        None => InterpretResult::CompileError,
    }
}

struct CallFrame {
    closure: memory::Gc<RefCell<object::ObjClosure>>,
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
    frames: memory::UniqueRoot<Vec<CallFrame>>,
    stack: memory::UniqueRoot<Vec<value::Value>>,
    globals: memory::UniqueRoot<HashMap<String, value::Value>>,
    open_upvalues: memory::UniqueRoot<Vec<memory::Gc<RefCell<object::ObjUpvalue>>>>,
}

impl Default for Vm {
    fn default() -> Self {
        Vm {
            frames: memory::allocate_unique(Vec::with_capacity(FRAMES_MAX)),
            stack: memory::allocate_unique(Vec::with_capacity(STACK_MAX)),
            globals: memory::allocate_unique(HashMap::new()),
            open_upvalues: memory::allocate_unique(Vec::new()),
        }
    }
}

fn clock_native(_arg_count: usize, _args: &mut [value::Value]) -> value::Value {
    let duration = time::SystemTime::now()
        .duration_since(time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let seconds = duration.as_secs_f64();
    let nanos = duration.subsec_nanos() as f64 / 1e9;
    value::Value::Number(seconds + nanos)
}

impl Vm {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn interpret(&mut self, function: memory::Root<object::ObjFunction>) -> InterpretResult {
        self.define_native("clock", clock_native);
        self.stack.push(value::Value::ObjFunction(function.as_gc()));

        let closure = memory::allocate(RefCell::new(object::ObjClosure::new(function.as_gc())));
        self.stack.pop();
        self.stack.push(value::Value::ObjClosure(closure.as_gc()));
        self.call_value(value::Value::ObjClosure(closure.as_gc()), 0);
        self.run()
    }

    fn run(&mut self) -> InterpretResult {
        let mut frame = self.frames.last_mut().unwrap();

        macro_rules! binary_op {
            ($value_type:expr, $op:tt) => {
                {
                    let second_value = self.stack.pop().unwrap();
                    let first_value = self.stack.pop().unwrap();
                    let (first, second) = match (first_value, second_value) {
                        (
                            value::Value::Number(first),
                            value::Value::Number(second)
                        ) => (first, second),
                        _ => {
                            self.runtime_error("Operands must be numbers.");
                            return InterpretResult::RuntimeError;
                        }
                    };
                    self.stack.push($value_type(first $op second));
                }
            };
        }

        macro_rules! read_byte {
            () => {{
                let ip = frame.ip;
                let ret = frame.closure.borrow().function.chunk.code[ip];
                frame.ip += 1;
                ret
            }};
        }

        macro_rules! read_short {
            () => {{
                let ret = ((frame.closure.borrow().function.chunk.code[frame.ip] as u16) << 8)
                    | frame.closure.borrow().function.chunk.code[frame.ip + 1] as u16;
                frame.ip += 2;
                ret
            }};
        }

        macro_rules! read_constant {
            () => {
                frame.closure.borrow().function.chunk.constants[read_byte!() as usize]
            };
        }

        macro_rules! read_string {
            () => {
                match read_constant!() {
                    value::Value::ObjString(s) => s,
                    _ => panic!("Expected variable name."),
                }
            };
        }

        loop {
            if cfg!(debug_assertions) {
                print!("          ");
                for v in self.stack.iter() {
                    print!("[ {} ]", v);
                }
                println!("");
                debug::disassemble_instruction(&frame.closure.borrow().function.chunk, frame.ip);
            }
            let instruction = chunk::OpCode::from(read_byte!());

            match instruction {
                chunk::OpCode::Constant => {
                    let constant = read_constant!();
                    self.stack.push(constant);
                }

                chunk::OpCode::Nil => {
                    self.stack.push(value::Value::None);
                }

                chunk::OpCode::True => {
                    self.stack.push(value::Value::Boolean(true));
                }

                chunk::OpCode::False => {
                    self.stack.push(value::Value::Boolean(false));
                }

                chunk::OpCode::Pop => {
                    self.stack.pop();
                }

                chunk::OpCode::GetLocal => {
                    let slot = read_byte!() as usize;
                    let value = self.stack[frame.slot_base + slot];
                    self.stack.push(value);
                }

                chunk::OpCode::SetLocal => {
                    let slot = read_byte!() as usize;
                    self.stack[frame.slot_base + slot] = *self.stack.last().unwrap();
                }

                chunk::OpCode::GetGlobal => {
                    let name = read_string!();
                    match self.globals.get(&name.data) {
                        Some(value) => {
                            self.stack.push(*value);
                        }
                        None => {
                            let msg = format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::DefineGlobal => {
                    let name = read_string!();
                    self.globals
                        .insert(name.data.clone(), *self.stack.last().unwrap());
                    self.stack.pop();
                }

                chunk::OpCode::SetGlobal => {
                    let name = read_string!();
                    let prev = self
                        .globals
                        .insert(name.data.clone(), *self.stack.last().unwrap());
                    match prev {
                        Some(_) => {}
                        None => {
                            self.globals.remove(&name.data);
                            let msg = format!("Undefined variable '{}'.", name.data);
                            self.runtime_error(msg.as_str());
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::GetUpvalue => {
                    let upvalue_index = read_byte!() as usize;
                    let upvalue = match *frame.closure.borrow().upvalues[upvalue_index].borrow() {
                        object::ObjUpvalue::Open(slot) => self.stack[slot],
                        object::ObjUpvalue::Closed(value) => value,
                    };
                    self.stack.push(upvalue);
                }

                chunk::OpCode::SetUpvalue => {
                    let upvalue = read_byte!() as usize;
                    match *frame.closure.borrow_mut().upvalues[upvalue].borrow_mut() {
                        object::ObjUpvalue::Open(slot) => {
                            self.stack[slot] = *self.stack.last().unwrap();
                        }
                        object::ObjUpvalue::Closed(ref mut value) => {
                            *value = *self.stack.last().unwrap();
                        }
                    };
                }

                chunk::OpCode::Equal => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    self.stack
                        .push(value::Value::Boolean(a.unwrap() == b.unwrap()));
                }

                chunk::OpCode::Greater => binary_op!(value::Value::Boolean, >),

                chunk::OpCode::Less => binary_op!(value::Value::Boolean, <),

                chunk::OpCode::Add => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    match (a.unwrap(), b.unwrap()) {
                        (value::Value::ObjString(a), value::Value::ObjString(b)) => self
                            .stack
                            .push(value::Value::from(format!("{}{}", a.data, b.data))),

                        (value::Value::Number(a), value::Value::Number(b)) => {
                            self.stack.push(value::Value::Number(a + b));
                        }

                        _ => {
                            self.runtime_error("Operands must be two numbers or two strings.");
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::Subtract => binary_op!(value::Value::Number, -),

                chunk::OpCode::Multiply => binary_op!(value::Value::Number, *),

                chunk::OpCode::Divide => binary_op!(value::Value::Number, /),

                chunk::OpCode::Not => {
                    let value = self.stack.pop().unwrap();
                    self.stack.push(value::Value::Boolean(!value.as_bool()));
                }

                chunk::OpCode::Negate => {
                    let value = self.stack.pop().unwrap();
                    match value {
                        value::Value::Number(underlying) => {
                            self.stack.push(value::Value::Number(-underlying));
                        }
                        _ => {
                            self.runtime_error("Operand must be a number.");
                            return InterpretResult::RuntimeError;
                        }
                    }
                }

                chunk::OpCode::Print => {
                    println!("{}", self.stack.pop().unwrap());
                }

                chunk::OpCode::Jump => {
                    let offset = read_short!();
                    frame.ip += offset as usize;
                }

                chunk::OpCode::JumpIfFalse => {
                    let offset = read_short!();
                    if !self.stack.last().unwrap().as_bool() {
                        frame.ip += offset as usize;
                    }
                }

                chunk::OpCode::Loop => {
                    let offset = read_short!();
                    frame.ip -= offset as usize;
                }

                chunk::OpCode::Call => {
                    let arg_count = read_byte!() as usize;
                    if !self.call_value(self.stack[self.stack.len() - 1 - arg_count], arg_count) {
                        return InterpretResult::RuntimeError;
                    }
                    frame = self.frames.last_mut().unwrap();
                }

                chunk::OpCode::Closure => {
                    let function = match read_constant!() {
                        value::Value::ObjFunction(underlying) => underlying,
                        _ => panic!("Expected ObjFunction."),
                    };

                    let upvalue_count = function.upvalue_count;

                    let closure = memory::allocate(RefCell::new(object::ObjClosure::new(function)));
                    self.stack.push(value::Value::ObjClosure(closure.as_gc()));

                    for i in 0..upvalue_count {
                        let is_local = read_byte!() != 0;
                        let index = read_byte!() as usize;
                        let slot_base = frame.slot_base;
                        closure.borrow_mut().upvalues[i] = if is_local {
                            capture_upvalue(self.open_upvalues.deref_mut(), slot_base + index)
                        } else {
                            frame.closure.borrow().upvalues[index]
                        };
                    }
                }

                chunk::OpCode::CloseUpvalue => {
                    close_upvalues(
                        &mut self.open_upvalues,
                        self.stack.len() - 1,
                        self.stack.last().unwrap(),
                    );
                    self.stack.pop();
                }

                chunk::OpCode::Return => {
                    let result = self.stack.pop().unwrap();
                    for i in frame.slot_base..self.stack.len() {
                        close_upvalues(&mut self.open_upvalues, i, &self.stack[i])
                    }

                    let prev_stack_size = frame.slot_base;
                    self.frames.pop();
                    if self.frames.is_empty() {
                        self.stack.pop();
                        return InterpretResult::Ok;
                    }

                    self.stack.truncate(prev_stack_size);
                    self.stack.push(result);

                    frame = self.frames.last_mut().unwrap();
                }
            }
        }
    }

    fn call_value(&mut self, value: value::Value, arg_count: usize) -> bool {
        match value {
            value::Value::ObjClosure(function) => {
                return self.call(function, arg_count);
            }

            value::Value::ObjNative(wrapped) => {
                let function = wrapped.function.unwrap();
                let frame_begin = self.stack.len() - arg_count - 1;
                let result = function(arg_count, &mut self.stack[frame_begin..]);
                self.stack.truncate(frame_begin);
                self.stack.push(result);
                return true;
            }

            _ => {
                self.runtime_error("Can only call functions and classes.");
                return false;
            }
        }
    }

    fn call(&mut self, closure: memory::Gc<RefCell<object::ObjClosure>>, arg_count: usize) -> bool {
        if arg_count as u32 != closure.borrow().function.arity {
            let msg = format!(
                "Expected {} arguments but got {}.",
                closure.borrow().function.arity,
                arg_count
            );
            self.runtime_error(msg.as_str());
            return false;
        }

        if self.frames.len() == FRAMES_MAX {
            self.runtime_error("Stack overflow.");
            return false;
        }

        self.frames.push(CallFrame {
            closure: closure,
            ip: 0,
            slot_base: self.stack.len() - arg_count - 1,
        });
        return true;
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
            if function.name.borrow().data.len() == 0 {
                eprintln!("script");
            } else {
                eprintln!("{}()", function.name.borrow().data);
            }
        }

        self.reset_stack();
    }

    fn define_native(&mut self, name: &str, function: object::NativeFn) {
        self.stack.push(value::Value::from(function));
        self.globals
            .insert(String::from(name), *self.stack.last().unwrap());
        self.stack.pop();
    }
}

fn capture_upvalue(
    open_upvalues: &mut Vec<memory::Gc<RefCell<object::ObjUpvalue>>>,
    location: usize,
) -> memory::Gc<RefCell<object::ObjUpvalue>> {
    let result = open_upvalues
        .iter()
        .find(|&u| u.borrow().is_open_with_index(location));

    let upvalue = if let Some(upvalue) = result {
        *upvalue
    } else {
        memory::allocate(RefCell::new(object::ObjUpvalue::new(location))).as_gc()
    };

    open_upvalues.push(upvalue);
    upvalue
}

fn close_upvalues(
    open_upvalues: &mut Vec<memory::Gc<RefCell<object::ObjUpvalue>>>,
    last: usize,
    value: &value::Value,
) {
    for upvalue in open_upvalues.iter() {
        if upvalue.borrow().is_open_with_index(last) {
            upvalue.borrow_mut().close(*value);
        }
    }

    open_upvalues.retain(|u| u.borrow().is_open());
}
