use katze_system2::message::{
    BreakCharacters, LayoutSettings, Page, StyleDefaults, TextEncoding, TimingSettings,
    VerticalAlignment,
};

fn prepare(page: &mut Page) {
    page.prepare(
        &LayoutSettings {
            encoding: TextEncoding::Cp932,
            width: 1000,
            margins: [0; 3],
            offset: [0; 2],
            character_spacing: 0,
            line_spacing: 0,
            normal_size: 20,
            ruby_offset: 0,
            vertical_alignment: VerticalAlignment::Preserve,
            line_breaking: false,
            word_wrap: false,
            hanging_indent: false,
            break_characters: BreakCharacters::default(),
        },
        &TimingSettings {
            encoding: TextEncoding::Cp932,
            time_scale: Some(10),
            character_delay: 3,
            period_delay: 8,
            skip_spaces: false,
        },
        |_, _| Ok(10),
    )
    .unwrap();
}

fn page(text: &str) -> Page {
    let mut page = Page::new(
        StyleDefaults {
            text_sizes: [20; 5],
            ruby_sizes: [12; 5],
            text_color: u32::MAX,
            ruby_color: u32::MAX,
            shadow_color: 0,
        },
        TextEncoding::Cp932,
    );
    page.append(text).unwrap();
    prepare(&mut page);
    page
}

#[test]
fn nonmonotonic_times_scan_all_glyphs_and_keep_prior_visibility() {
    let mut page = page(r"A\wf4;B\wrCD");
    let first = page.advance(0, u32::MAX).unwrap();
    assert_eq!(first.newly_visible, [0, 3]);
    assert!(!first.complete);
    assert_eq!(page.remaining(u32::MAX).unwrap(), 2);
    let next = page.advance(3, u32::MAX).unwrap();
    assert_eq!(next.newly_visible, [4]);
    assert!(!next.complete);
    let last = page.advance(7, u32::MAX).unwrap();
    assert_eq!(last.newly_visible, [1]);
    assert!(last.complete);

    let backwards = page.advance(0, u32::MAX).unwrap();
    assert!(!backwards.complete);
    assert!(backwards.newly_visible.is_empty());
    assert_eq!(page.remaining(u32::MAX).unwrap(), 0);
    assert_eq!(page.is_visible(4), Some(true));
    assert_eq!(page.is_visible(2), Some(false));
    assert_eq!(page.is_visible(5), None);
}

#[test]
fn speed_changes_rebase_once_and_append_preserves_visible_atoms() {
    let mut page = page("ABC");
    assert_eq!(page.advance(2, u32::MAX).unwrap().newly_visible, [0]);
    page.set_speed(20);
    assert!(page.advance(3, u32::MAX).unwrap().newly_visible.is_empty());
    assert_eq!(page.advance(4, u32::MAX).unwrap().newly_visible, [1]);
    page.set_speed(20);
    assert_eq!(page.advance(5, u32::MAX).unwrap().newly_visible, [2]);
    page.append("D").unwrap();
    assert!(page.advance(7, u32::MAX).is_err());
    assert!(page.remaining(u32::MAX).is_err());
    prepare(&mut page);
    assert_eq!(page.advance(7, u32::MAX).unwrap().newly_visible, [3]);
    assert_eq!(page.remaining(u32::MAX).unwrap(), 0);

    page.clear();
    assert_eq!(page.speed(), 20);
    page.append("ABC").unwrap();
    prepare(&mut page);
    assert_eq!(page.advance(0, u32::MAX).unwrap().newly_visible, [0]);
    page.set_speed(0);
    assert!(!page.advance(100, u32::MAX).unwrap().complete);
    let force = page.advance(u32::MAX, u32::MAX).unwrap();
    assert_eq!(force.newly_visible, [1, 2]);
    assert!(force.complete);
}

#[test]
fn segment_limits_preserve_the_clock_when_scanning_stops_early() {
    let mut page = page("A\u{ffff}B\u{ffff}C");
    let first = page.advance(4, 0).unwrap();
    assert_eq!(first.newly_visible, [0]);
    assert!(first.complete);
    assert_eq!(page.remaining(0).unwrap(), 0);
    assert_eq!(page.remaining(1).unwrap(), 1);
    assert_eq!(page.remaining(u32::MAX).unwrap(), 2);

    page.set_speed(20);
    let second = page.advance(2, 1).unwrap();
    assert_eq!(second.newly_visible, [2]);
    assert!(second.complete);
    assert_eq!(page.remaining(1).unwrap(), 0);
    page.set_speed(30);
    let third = page.advance(1, u32::MAX).unwrap();
    assert_eq!(third.newly_visible, [4]);
    assert!(!third.complete);
    assert_eq!(page.remaining(u32::MAX).unwrap(), 0);
    assert!(
        page.advance(u32::MAX, u32::MAX)
            .unwrap()
            .newly_visible
            .is_empty()
    );
}
