use super::{ObjectState, Objects};
use crate::{Error, Result, expression::State, fes};

impl Objects {
    pub(super) fn dispatch_query_command(
        &self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        numbers: &mut State,
    ) -> Result<bool> {
        let command = args[1].as_slice();
        if command == b"getpos2" && target.bank == -1 {
            return Ok(true);
        }
        if command == b"lineinfo" && args.get(2).is_none() {
            return Ok(true);
        }
        let count = if command == b"endpos" { 2 } else { 3 };
        let mut values = [if command == b"lineinfo" { 0 } else { -1 }; 3];
        for (value, bytes) in values.iter_mut().take(count).zip(args.iter().skip(2)) {
            *value = interpreter
                .eval(bytes, numbers)
                .map_err(|error| Error::new(offset + error.offset, error.message))?;
        }
        // 行内容只由日志类型提供，普通对象保留上述参数求值。
        if command == b"lineinfo" {
            return Ok(true);
        }
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                continue;
            };
            let object = self.get(handle).unwrap();
            let position = if command == b"endpos" {
                let ObjectState::String(state) = &object.state else {
                    continue;
                };
                let [x, y] = state.cursor();
                [x, y, 0]
            } else if object.state.plane().is_some() {
                let point = self.plane_position(handle);
                std::array::from_fn(|i| super::plane::integer_position(point[i]))
            } else if object.state.image().is_some() {
                let [x, y] = self
                    .button_position(handle)
                    .map(super::plane::integer_position);
                [x, y, 0]
            } else {
                continue;
            };
            for (&index, value) in values.iter().zip(position).take(count) {
                if let Some(slot) = numbers.shared.get_mut(index as usize) {
                    *slot = value;
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_position_queries_limit_parent_types_and_preserve_array_and_axis_order() {
        for (parent_kind, expected) in [("STRING", [6, -6, 2]), ("STRING8", [8, -9, 3])] {
            let mut objects = Objects::new(Default::default()).unwrap();
            let children = objects
                .define(&fes::ObjectDefinition {
                    kind: "STRING8",
                    name: "item".into(),
                    banks: 2,
                })
                .unwrap();
            let parent = objects
                .define(&fes::ObjectDefinition {
                    kind: parent_kind,
                    name: "parent".into(),
                    banks: 1,
                })
                .unwrap()[0];
            let grand = objects
                .define(&fes::ObjectDefinition {
                    kind: "CPLANE",
                    name: "grand".into(),
                    banks: 1,
                })
                .unwrap()[0];
            objects.set_parent(children[0], Some(parent)).unwrap();
            objects.set_parent(parent, Some(grand)).unwrap();
            for (handle, position) in [
                (children[0], [8.75, -9.75, 3.5, 1.]),
                (parent, [-3.25, 4.25, -1.5, 1.]),
                (grand, [0.5, -0.5, 0.25, 1.]),
            ] {
                objects
                    .get_mut(handle)
                    .unwrap()
                    .state
                    .plane_mut()
                    .unwrap()
                    .position = position;
            }
            let mut interpreter = fes::Interpreter::new(
                fes::Program::from_text(b"#DEFINE\nSTRING8 item[2]\n#START\nwait 99\n").unwrap(),
            )
            .unwrap();
            objects.publish_bindings(&mut interpreter);
            let mut numbers = State::new(8);
            let mut send = |objects: &mut Objects, input: &[u8]| {
                assert!(
                    objects
                        .dispatch_object_command(
                            &mut interpreter,
                            &fes::Line {
                                offset: 0,
                                bytes: input.to_vec()
                            },
                            &mut numbers
                        )
                        .unwrap()
                );
            };
            send(&mut objects, b"item[0] getpos2 0 1 2");
            send(&mut objects, b"item[0] getpos2 2 2 2");
            send(&mut objects, b"item getpos2 ($7+=1) ($7+=1) ($7+=1)");
            assert_eq!(numbers.shared[..3], expected);
            assert_eq!(numbers.shared[7], 0);
            objects
                .get_mut(children[0])
                .unwrap()
                .state
                .plane_mut()
                .unwrap()
                .position = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.];
            assert!(
                objects
                    .dispatch_object_command(
                        &mut interpreter,
                        &fes::Line {
                            offset: 0,
                            bytes: b"item[0] getpos2 0 1 2".to_vec()
                        },
                        &mut numbers
                    )
                    .unwrap()
            );
            assert_eq!(numbers.shared[..3], [i32::MIN; 3]);
        }
    }
}
