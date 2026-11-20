use super::Scene;
use crate::{
    Error, Result, fes,
    host::resources::ResourceMode,
    kcs::{SequenceGroup, SequenceMap},
    runtime::{ObjectFactory, Store},
    save::DataContainer,
};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_gallery_command(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        let result = match args {
            [command, file, key, list, ..] if command == b"cgreg_init" => {
                self.load_fes_gallery(owner, scene, file, key, list)
            }
            [command, group, dest, ..] if command == b"cgregnum" => {
                self.count_fes_gallery_images(scene, group, dest)
            }
            [target, command, list, group, row, frame, ..] if command == b"loadlist2" => {
                self.load_fes_gallery_image(scene, line, target, [list, group, row, frame])
            }
            _ => return Ok(false),
        };
        result
            .map(|()| true)
            .map_err(|error| Error::new(line.offset + error.offset, error.message))
    }

    fn load_fes_gallery(
        &mut self,
        owner: u32,
        scene: &mut Scene,
        file: &[u8],
        key: &[u8],
        list: &[u8],
    ) -> Result<()> {
        let key = scene.interpreter.eval(key, self.numbers)? as u32;
        let mut register = SequenceMap::default();
        let saves = self
            .host
            .save_services
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置鉴赏存档读取服务"))?;
        if let Some(file) = saves.read(owner, file)? {
            if let Some(data) = DataContainer::from_css(&file)? {
                if let Some(record) = data.record(key)? {
                    register.restore(record)?;
                }
            }
        }
        let resources = self
            .resources
            .services
            .as_mut()
            .ok_or_else(|| Error::new(0, "运行时未配置鉴赏列表资源服务"))?;
        let name = resources.resolve(b"config", list)?;
        if let Some(bytes) = resources.read(&name, ResourceMode::LooseFirst)? {
            register.load(bytes)?;
        }
        scene.gallery = register;
        Ok(())
    }

    fn count_fes_gallery_images(
        &mut self,
        scene: &mut Scene,
        group: &[u8],
        dest: &[u8],
    ) -> Result<()> {
        let group = self.gallery_group(scene, 0, group)?;
        let dest = scene.interpreter.eval(dest, self.numbers)?;
        let mut count = 0;
        scene.gallery.for_each_seen(group, |_| {
            count += 1;
            true
        })?;
        if let Some(slot) = self.numbers.shared.get_mut(dest as usize) {
            *slot = count;
        }
        Ok(())
    }

    fn gallery_group<'a>(
        &mut self,
        scene: &mut Scene,
        list: u32,
        source: &'a [u8],
    ) -> Result<SequenceGroup<'a>> {
        if matches!(source.first(), Some(b'$' | b'\\' | b'0'..=b'9')) {
            Ok(SequenceGroup::Index(
                scene.interpreter.eval(source, self.numbers)? as u32,
            ))
        } else {
            Ok(SequenceGroup::Named { list, name: source })
        }
    }

    fn load_fes_gallery_image(
        &mut self,
        scene: &mut Scene,
        line: &fes::Line,
        target: &[u8],
        [list, group, row, frame]: [&[u8]; 4],
    ) -> Result<()> {
        let mut target = scene
            .interpreter
            .command_object_reference(target, self.numbers)?;
        if scene.objects.fallback_missing_target(
            &mut scene.interpreter,
            &target,
            &line.bytes,
            self.numbers,
        )? {
            return Ok(());
        }
        let list = scene.interpreter.eval(list, self.numbers)? as u32;
        let group = self.gallery_group(scene, list, group)?;
        let mut row = scene.interpreter.eval(row, self.numbers)?;
        let frame = scene.interpreter.eval(frame, self.numbers)?;
        if row < 0 {
            return Ok(());
        }
        let mut file = None;
        scene.gallery.for_each_seen(group, |name| {
            if row == 0 {
                file = Some(name.to_vec());
                return false;
            }
            row -= 1;
            true
        })?;
        let Some(file) = file else {
            return Ok(());
        };
        let count = scene.objects.count(&target.name);
        if count == 1 {
            target.bank = 0;
        }
        let banks = if target.bank == -1 {
            0..i64::from(count as i32)
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let attributes = fes::ObjectAttributes {
            file: Some(file),
            image_number: Some(frame),
            ..Default::default()
        };
        for bank in banks {
            let Some(handle) = scene.objects.lookup(&target.name, bank as i32) else {
                break;
            };
            let state = &mut scene.objects.get_mut(handle).unwrap().state;
            if let Some(image) = state.image_source_mut() {
                image.apply(&attributes, bank as u32, |name| {
                    self.host.languages.expand_image(name)
                })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::State;
    use crate::host::{resources::ResourceServices, storage::SaveServices};
    use crate::runtime::{BaseObject, Runtime, TaskObject, kind};

    struct NoTasks;

    impl ObjectFactory for NoTasks {
        fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
            Err(Error::new(0, "unexpected child task"))
        }
    }

    struct List(Vec<u8>);

    impl ResourceServices for List {
        fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(category, b"config");
            Ok(name.to_vec())
        }

        fn read(&mut self, name: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            assert_eq!(name, b"gallery.dat");
            Ok(Some(self.0.clone()))
        }
    }

    struct Save(Vec<u8>);

    impl SaveServices for Save {
        fn read(&mut self, _: u32, name: &[u8]) -> Result<Option<Vec<u8>>> {
            assert_eq!(name, b"progress.dat");
            Ok(Some(self.0.clone()))
        }
    }

    #[test]
    fn gallery_counts_and_loads_only_seen_entries_in_list_order() -> Result<()> {
        let mut saved_record = Vec::new();
        for name in [b"second".as_slice(), b"first"] {
            saved_record.extend_from_slice(&1u32.to_le_bytes());
            let start = saved_record.len();
            saved_record.extend_from_slice(name);
            saved_record.resize(start + 64, 0);
            saved_record.extend_from_slice(&0u32.to_le_bytes());
            saved_record.extend_from_slice(&(-1i32).to_le_bytes());
        }
        let mut save = DataContainer::new(1);
        save.append(50, &saved_record)?;

        let mut list = vec![0; 12 + 72];
        list[8..12].copy_from_slice(&1u32.to_le_bytes());
        list[16..19].copy_from_slice(b"row");
        list[80..84].copy_from_slice(&3u32.to_le_bytes());
        for name in [b"first".as_slice(), b"locked", b"second"] {
            let start = list.len();
            list.extend_from_slice(name);
            list.resize(start + 64, 0);
        }
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(8),
            NoTasks,
        );
        runtime.set_save_services(Save(save.to_css()?));
        runtime.set_resources(List(list));
        let mut scene = Scene::new(
            fes::Program::from_text(b"#DEFINE\nIMAGE picture\n#START\nwait 1\n")?,
            runtime.host().handle_allocator(),
        )?;
        assert!(scene.prepare(runtime.numbers_mut(), 16)?);
        for text in [
            b"cgreg_init progress.dat 50 gallery.dat".as_slice(),
            b"cgregnum 0 1",
            b"picture loadlist2 0 0 1 7",
            b"picture loadlist2 0 row 2 9",
        ] {
            let line = fes::Line {
                offset: 0,
                bytes: text.to_vec(),
            };
            let args = scene.interpreter.parameters(text, runtime.numbers())?;
            assert!(
                runtime
                    .store()
                    .fes_gallery_command(0, &mut scene, &args, &line)?
            );
        }
        let handle = scene.objects.lookup("picture", 0).unwrap();
        let image = scene
            .objects
            .get(handle)
            .unwrap()
            .state
            .image_source()
            .unwrap();
        assert_eq!(runtime.numbers().shared[1], 2);
        assert_eq!(image.requested_file, b"second");
        assert_eq!(image.image_number, 7);
        Ok(())
    }
}
