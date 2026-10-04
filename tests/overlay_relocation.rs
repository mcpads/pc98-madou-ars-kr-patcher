use std::collections::{HashMap, HashSet};

use pc98_madou_ars::font_build::{FontProfile, build_renderer_font, collect_renderer_demand};
use pc98_madou_ars::hook_geometry::RendererOverlay;
use pc98_madou_ars::josa::{OVERLAY_STUB_LEN, install_topic_particle_selector_at};
use pc98_madou_ars::overlay_batch::apply_translations_in_source_slots;
use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
use pc98_madou_ars::overlay_messages::unified_catalog;
use pc98_madou_ars::overlay_reloc::SheetCodes;
use pc98_madou_ars::received_damage_sfx::{
    S0_DAMAGE_OVERLAY_STUB_BYTES, install_s0_damage_route_at,
};

#[path = "common/mod.rs"]
mod common;

fn parse_hex(text: &str) -> usize {
    usize::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
}

fn staged_overlay_translations(overlay: &str) -> HashMap<usize, String> {
    let mut paths = std::fs::read_dir("assets/translations/needs_review")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut translations = HashMap::new();
    for path in paths {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if value.get("overlay").and_then(|item| item.as_str()) != Some(overlay) {
            continue;
        }
        for entry in value["entries"].as_array().unwrap() {
            let offset = parse_hex(entry["string_decoded_offset"].as_str().unwrap());
            let korean = entry["ko"].as_str().unwrap();
            assert!(
                !korean.is_empty(),
                "{} has an empty translation",
                path.display()
            );
            assert!(
                translations.insert(offset, korean.to_owned()).is_none(),
                "{overlay} has duplicate translation offset 0x{offset:04X}",
            );
        }
    }
    translations
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn staged_dynamic_particle_suffixes_keep_runtime_selection() {
    let forms = ["{josa:을}", "{josa:이}", "{josa:은}", "{josa:와}"];
    let mut counts = [0usize; 4];
    let mut paths = std::fs::read_dir("assets/translations/needs_review")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if !matches!(
            value.get("overlay").and_then(|item| item.as_str()),
            Some("GAME_A.OVL" | "GAME_R.OVL" | "GAME_S.OVL")
        ) {
            continue;
        }
        for entry in value["entries"].as_array().unwrap() {
            let korean = entry["ko"].as_str().unwrap();
            for (index, marker) in forms.iter().enumerate() {
                let count = korean.matches(marker).count();
                if count == 0 {
                    continue;
                }
                assert_eq!(
                    count,
                    1,
                    "{} {} repeats one dynamic particle marker",
                    path.display(),
                    entry["id"].as_str().unwrap(),
                );
                let korean_join =
                    korean.trim_start_matches(|ch: char| ch.is_control() || ch.is_whitespace());
                assert!(
                    korean_join.starts_with(marker),
                    "{} {} places {marker} after visible Korean text",
                    path.display(),
                    entry["id"].as_str().unwrap(),
                );
                counts[index] += count;
            }
        }
    }

    assert!(
        counts[0] > 0,
        "staged translations exercise 을/를 selection"
    );
    assert!(
        counts[1] > 0,
        "staged translations exercise 이/가 selection"
    );
    assert!(
        counts[2] > 0,
        "staged translations exercise 은/는 selection"
    );
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn real_overlay_catalogs_and_staged_relocations_are_fail_closed() {
    // Source-media catalog shape and fixed dynamic-topic patch sites are stable.
    // Translation-dependent placement counts and byte use are checked against
    // behavioral bounds below instead of being frozen as snapshot values.
    let expected = [
        (
            "GAME_A.OVL",
            "arle",
            367usize,
            [0x112Dusize, 0x1352usize],
            RendererOverlay::Arle,
        ),
        (
            "GAME_R.OVL",
            "rulue",
            385,
            [0x1179, 0x139E],
            RendererOverlay::Rulue,
        ),
        (
            "GAME_S.OVL",
            "schezo",
            326,
            [0x1156, 0x137B],
            RendererOverlay::Schezo,
        ),
    ];

    for (file, character, message_count, topic_sites, overlay) in expected {
        let Some(packed) = common::try_read(&format!("research/extracted/{file}")) else {
            continue;
        };
        let mut decoded = decode_overlay_lz(&packed).unwrap().output;
        let source_decoded_len = decoded.len();
        let messages = unified_catalog(&decoded, 0x100, 4, &[2, 4, 6, 8, 10, 12, 14, 16]).unwrap();
        assert_eq!(messages.len(), message_count, "{file} message count");

        // Regression for the exact GAME_S false positive: these were SJIS
        // `82 BE`, NUL, and the next lead byte, never mov-si immediates.
        if file == "GAME_S.OVL" {
            assert!(
                messages
                    .iter()
                    .all(|message| message.string_logical_offset != 0x8200)
            );
            let forbidden = HashSet::from([0x12EFusize, 0x6FC5, 0x9A7A]);
            assert!(messages.iter().all(|message| {
                message
                    .rewrite_sites
                    .iter()
                    .all(|site| !forbidden.contains(&site.site))
            }));
        }

        if file == "GAME_R.OVL" {
            assert!(
                messages.iter().all(|message| {
                    message.rewrite_sites.iter().all(|site| site.site != 0x569E)
                })
            );
            let cloud_tree_fruit = messages
                .iter()
                .find(|message| message.string_decoded_offset == 0x6A39)
                .expect("GAME_R item-table label");
            assert_eq!(
                cloud_tree_fruit
                    .rewrite_sites
                    .iter()
                    .map(|site| site.site)
                    .collect::<Vec<_>>(),
                vec![0x5FE2, 0xAD97],
                "GAME_R item-table and direct consumer must move together",
            );
            assert!(messages.iter().any(|message| {
                message.string_decoded_offset == 0x6A29
                    && message.rewrite_sites.iter().any(|site| site.site == 0x5FCE)
            }));
            assert!(messages.iter().any(|message| {
                message.string_decoded_offset == 0x6A30
                    && message.rewrite_sites.iter().any(|site| site.site == 0x5FD8)
            }));
        }

        let staged = staged_overlay_translations(file);
        let known = messages
            .iter()
            .map(|message| message.string_decoded_offset)
            .collect::<HashSet<_>>();
        let cataloged = staged
            .iter()
            .filter(|(offset, _)| known.contains(offset))
            .map(|(offset, korean)| (*offset, korean.clone()))
            .collect::<HashMap<_, _>>();
        assert_eq!(cataloged.len(), message_count, "{file} catalog coverage");
        assert!(
            staged.len() >= cataloged.len(),
            "{file} staged corpus lost cataloged messages"
        );

        let profile =
            FontProfile::load(std::path::Path::new("assets/fonts/font_profile.json")).unwrap();
        let demand = collect_renderer_demand(
            std::path::Path::new("assets/translations/needs_review"),
            character,
            true,
        )
        .unwrap();
        let generated = build_renderer_font(&profile, &demand).unwrap();
        let sheet = SheetCodes::from_json_str(&serde_json::to_string(&generated.metadata).unwrap())
            .unwrap();
        let mut stub_sizes = vec![OVERLAY_STUB_LEN, S0_DAMAGE_OVERLAY_STUB_BYTES];
        if overlay == RendererOverlay::Rulue {
            stub_sizes.push(pc98_madou_ars::failure_message::STUB_BYTES);
        }
        if overlay != RendererOverlay::Rulue {
            stub_sizes.push(pc98_madou_ars::illusion::STUB_BYTES);
        }
        stub_sizes.push(pc98_madou_ars::dancer_thunder::STUB_BYTES);
        let report = apply_translations_in_source_slots(
            &mut decoded,
            0x100,
            &messages,
            &cataloged,
            &sheet,
            &stub_sizes,
        )
        .unwrap();
        // Equipping the elephant must render the same relocated name shown
        // in inventory, including when the old equipment record is removed.
        if file == "GAME_A.OVL" {
            assert_eq!(
                &decoded[0x5E4A..0x5E4C],
                &decoded[0x9B6E..0x9B70],
                "equipment feedback must follow the relocated inventory name",
            );
        }
        // B5 orb use compares its inventory name address as item identity.
        // Relocating the Korean label must preserve equality at that consumer.
        if file == "GAME_S.OVL" {
            assert_eq!(
                &decoded[0x5E42..0x5E44],
                &decoded[0x7865..0x7867],
                "relocated orb must still pass the B5 item-use identity check",
            );
        }
        // These approved names outgrow their original slots. The inventory
        // pointer must reach the complete encoded text without growing GAME.
        for message in messages
            .iter()
            .filter(|m| matches!(m.text.as_str(), "犬顎菊" | "卵酒"))
        {
            let korean = &cataloged[&message.string_decoded_offset];
            let encoded = sheet.encode_line(korean).unwrap();
            assert!(encoded.len() + 1 > message.byte_budget);
            for site in &message.rewrite_sites {
                let destination = u16::from_le_bytes([decoded[site.site], decoded[site.site + 1]])
                    as usize
                    - 0x100;
                assert_ne!(destination, message.string_decoded_offset);
                assert_eq!(&decoded[destination..destination + encoded.len()], encoded);
                assert_eq!(decoded[destination + encoded.len()], 0);
            }
        }
        // Status-effect expiry heads share the suffix `の　効果が切れた`
        // with separately rendered suffix messages. The field immediate must
        // follow the complete Korean head, and no Japanese effect name may be
        // left in front of whatever now occupies the source suffix bytes.
        let expiry_head = match file {
            "GAME_A.OVL" => (0xA66Cusize, 0x90ECusize),
            "GAME_R.OVL" => (0xAF16, 0x9DC1),
            _ => (0x9911, 0x8467),
        };
        {
            let (head, site) = expiry_head;
            let korean = &cataloged[&head];
            let encoded = sheet.encode_line(korean).unwrap();
            let destination =
                u16::from_le_bytes([decoded[site], decoded[site + 1]]) as usize - 0x100;
            assert_eq!(
                &decoded[destination..destination + encoded.len()],
                encoded.as_slice(),
                "{file} status-effect expiry field must render its complete Korean head",
            );
            assert_eq!(decoded[destination + encoded.len()], 0);
        }
        // Instruction bytes misread as `韵げ` and the Rulue name-record link
        // word misread as `罵` must stay source-exact.
        if file == "GAME_A.OVL" {
            assert_eq!(&decoded[0x966E..0x9673], &[0xE8, 0xEF, 0x82, 0xB0, 0x00]);
        }
        if file == "GAME_R.OVL" {
            assert_eq!(&decoded[0x6A57..0x6A59], &[0x94, 0x6C]);
        }
        if file == "GAME_R.OVL" {
            assert_eq!(
                &decoded[0x5699..0x56A2],
                &[0x2E, 0x89, 0x0E, 0x02, 0xBE, 0x57, 0xBD, 0x6E, 0xBE],
                "GAME_R graphics setup instructions must not be rewritten as a false mov-si",
            );
        }
        assert_eq!(
            report.in_place + report.relocated,
            cataloged.len(),
            "{file} applied translation count"
        );
        assert!(
            report.slot_bytes_used <= report.slot_capacity,
            "{file} source-slot plan exceeds proven text capacity"
        );
        assert_eq!(report.untranslated, 0, "{file} untranslated catalog");
        let before_save_fix = decoded.clone();
        let mut wrong_callback = decoded.clone();
        wrong_callback[0x168] ^= 1;
        let wrong_before = wrong_callback.clone();
        assert!(pc98_madou_ars::save_encounter::install(&mut wrong_callback, overlay).is_err());
        assert_eq!(
            wrong_callback, wrong_before,
            "{file} failed guard mutated input"
        );
        pc98_madou_ars::save_encounter::install(&mut decoded, overlay).unwrap();
        let changed = before_save_fix
            .iter()
            .zip(&decoded)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect::<Vec<_>>();
        assert_eq!(
            changed,
            [0x15D, 0x162],
            "{file} save fix changed unrelated bytes"
        );
        assert_eq!(
            &decoded[0x15C..0x163],
            &[0x26, 0xC6, 0x87, 0xF0, 0x03, 0x51, 0x90]
        );
        let topic =
            install_topic_particle_selector_at(&mut decoded, 0x100, report.reservations[0].offset)
                .unwrap();
        assert_eq!(topic.sites, topic_sites, "{file} dynamic topic sites");
        assert_eq!(
            topic.stub_decoded_offset, report.reservations[0].offset,
            "{file} topic stub uses its source-slot reservation",
        );
        let damage =
            install_s0_damage_route_at(&mut decoded, overlay, 0x100, report.reservations[1].offset)
                .unwrap();
        assert_eq!(
            damage.stub_decoded_offset, report.reservations[1].offset,
            "{file} damage stub uses its source-slot reservation",
        );
        assert_eq!(
            decoded.len(),
            source_decoded_len,
            "{file} source-slot build changed the decoded GAME extent",
        );

        if overlay == RendererOverlay::Rulue {
            let slot = report.reservations[2].offset;
            let before = decoded.clone();
            let mut wrong = decoded.clone();
            wrong[0xAC19] ^= 1;
            let rejected = wrong.clone();
            assert!(pc98_madou_ars::failure_message::install(&mut wrong, slot).is_err());
            assert_eq!(wrong, rejected, "guard failure must be atomic");
            pc98_madou_ars::failure_message::install(&mut decoded, slot).unwrap();
            for (offset, (a, b)) in before.iter().zip(&decoded).enumerate() {
                if a != b {
                    assert!(
                        (0xABFB..0xABFE).contains(&offset) || (slot..slot + 22).contains(&offset)
                    );
                }
            }
            let call_disp = i16::from_le_bytes([decoded[0xABFC], decoded[0xABFD]]);
            assert_eq!(
                0xACFE_u16.wrapping_add_signed(call_disp),
                (slot + 0x100) as u16
            );
            let jump_disp = i16::from_le_bytes([decoded[slot + 20], decoded[slot + 21]]);
            assert_eq!(
                ((slot + 0x100 + 22) as u16).wrapping_add_signed(jump_disp),
                0x367C
            );
            assert_eq!(
                &decoded[0xAC0E..0xAC1E],
                &before[0xAC0E..0xAC1E],
                "failure setup remains intact"
            );
            assert_eq!(
                &decoded[0x3115..0x314E],
                &before[0x3115..0x314E],
                "failure consumption remains intact"
            );
        }
        if overlay != RendererOverlay::Rulue {
            let slot = report.reservations[2].offset;
            let entry = if overlay == RendererOverlay::Arle {
                0x9277
            } else {
                0x85f2
            };
            let before = decoded.clone();
            let mut wrong = decoded.clone();
            wrong[entry + 30] ^= 1;
            let rejected = wrong.clone();
            assert!(pc98_madou_ars::illusion::install(&mut wrong, overlay, slot).is_err());
            assert_eq!(wrong, rejected);
            pc98_madou_ars::illusion::install(&mut decoded, overlay, slot).unwrap();
            for (offset, (a, b)) in before.iter().zip(&decoded).enumerate() {
                if a != b {
                    assert!(
                        (entry..entry + 4).contains(&offset)
                            || (slot..slot + pc98_madou_ars::illusion::STUB_BYTES)
                                .contains(&offset)
                    );
                }
            }
            let rel = i16::from_le_bytes([decoded[entry + 1], decoded[entry + 2]]);
            assert_eq!(
                ((entry + 0x103) as u16).wrapping_add_signed(rel),
                (slot + 0x100) as u16
            );
        }
        let before_sale = decoded.clone();
        if overlay == RendererOverlay::Rulue {
            let mut wrong = decoded.clone();
            wrong[0x7E11] ^= 1;
            let rejected = wrong.clone();
            assert!(pc98_madou_ars::shop_sale::install(&mut wrong, overlay).is_err());
            assert_eq!(wrong, rejected);
        }
        pc98_madou_ars::shop_sale::install(&mut decoded, overlay).unwrap();
        if overlay == RendererOverlay::Rulue {
            // ES:[DI] -> AL; clear inventory flags/AH; full-width index*10.
            assert_eq!(
                &decoded[0x7E07..0x7E11],
                &[0x26, 0x8A, 0x05, 0x25, 0x3F, 0, 0x6B, 0xF0, 10, 0x90]
            );
            assert_eq!(&decoded[..0x7E07], &before_sale[..0x7E07]);
            assert_eq!(&decoded[0x7E11..], &before_sale[0x7E11..]);
        } else {
            assert_eq!(decoded, before_sale, "A/S already use full-width MUL");
        }
        let slot = report.reservations.last().unwrap().offset;
        let before = decoded.clone();
        let call = match overlay {
            RendererOverlay::Arle => 0x96A8,
            RendererOverlay::Rulue => 0xA2CB,
            RendererOverlay::Schezo => 0x8A4A,
        };
        let mut wrong = decoded.clone();
        wrong[call + 11] ^= 1;
        let rejected = wrong.clone();
        assert!(pc98_madou_ars::dancer_thunder::install(&mut wrong, overlay, slot).is_err());
        assert_eq!(wrong, rejected);
        pc98_madou_ars::dancer_thunder::install(&mut decoded, overlay, slot).unwrap();
        for (offset, (a, b)) in before.iter().zip(&decoded).enumerate() {
            if a != b {
                assert!(
                    (call..call + 3).contains(&offset)
                        || (call + 8..call + 13).contains(&offset)
                        || (slot..slot + 8).contains(&offset)
                );
            }
        }
        assert_eq!(
            &decoded[call + 8..call + 13],
            &[0x26, 0x89, 0x87, 0x08, 0x03]
        );
        let redirected = i16::from_le_bytes([decoded[call + 1], decoded[call + 2]]);
        assert_eq!(
            ((call + 0x103) as u16).wrapping_add_signed(redirected),
            (slot + 0x100) as u16
        );
        let setup = i16::from_le_bytes([decoded[slot + 1], decoded[slot + 2]]);
        let original_setup = match overlay {
            RendererOverlay::Arle => 0x3683,
            RendererOverlay::Rulue => 0x367C,
            RendererOverlay::Schezo => 0x3623,
        };
        assert_eq!(
            ((slot + 0x103) as u16).wrapping_add_signed(setup),
            original_setup
        );
        assert_eq!(&decoded[slot + 3..slot + 8], &[0xB4, 1, 0xCD, 0x7B, 0xC3]);
        let reencoded = encode_overlay_lz(&decoded);
        assert_eq!(
            decode_overlay_lz(&reencoded).unwrap().output,
            decoded,
            "{file} translated codec round-trip",
        );
    }
}
