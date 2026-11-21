use super::{ObjectFactory, ObjectHost};
use crate::Result;
use crate::kcs::{Interpreter, NativeReturn, TableHandler};

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn table_native(
        &mut self,
        handler: TableHandler,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        let services = self
            .resources
            .services
            .as_mut()
            .map(|services| services.as_mut() as _);
        match handler {
            TableHandler::NameTable => self.resources.raw.name_table_call(
                index,
                arguments,
                vm,
                services,
                self.host.unicode_text != 0,
            ),
            TableHandler::SectionTable => self
                .resources
                .raw
                .section_table_call(index, arguments, vm, services),
            TableHandler::DatabaseTable => self
                .resources
                .raw
                .database_table_call(index, arguments, vm, services),
            TableHandler::QuestTable => {
                let bytes = crate::kcs::load_quest_table(arguments, vm, services)?;
                let handle = match bytes {
                    Some(bytes) => self.host.create_quest_table(&bytes)?,
                    None => 0,
                };
                Ok(NativeReturn::Value(handle))
            }
            TableHandler::Quiz => self.resources.raw.quiz_call(
                index,
                arguments,
                vm,
                services,
                Some(&mut self.host.random),
            ),
            TableHandler::QuizOptions => self.resources.raw.quiz_option_call(
                index,
                arguments,
                vm,
                services,
                Some(&mut self.host.random),
            ),
            TableHandler::SequenceFile => {
                if let Some((handle, bytes)) =
                    crate::kcs::load_sequence_file(arguments, vm, services)?
                {
                    self.host.load_sequence_data(handle, bytes)?;
                }
                Ok(NativeReturn::Continue)
            }
            TableHandler::DatabaseLoad => {
                let database = crate::kcs::load_database(arguments, vm, services)?;
                Ok(NativeReturn::Value(self.host.create_database(database)?))
            }
        }
    }
}
