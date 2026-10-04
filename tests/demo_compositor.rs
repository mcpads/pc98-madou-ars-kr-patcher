use pc98_madou_ars::demo_compositor::{ByteSpan, Rect, audit_demo_compositor};

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";

fn real_audit() -> Option<pc98_madou_ars::demo_compositor::DemoCompositorAudit> {
    let disk = common::try_read(DEMO_DISK)?;
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL").unwrap();
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(decoded.bytes_consumed, packed.len());
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
        "4321b330cd4eb3d57880fbd2b0189d78ba56f5e204013f2fb0e66d502656b1ef"
    );
    Some(audit_demo_compositor(&decoded.output).unwrap())
}

fn has_source_rect(
    audit: &pc98_madou_ars::demo_compositor::DemoCompositorAudit,
    resource: &str,
    call_offset: u16,
    rect: Rect,
) -> bool {
    audit.blits.iter().any(|blit| {
        blit.resource == Some(resource)
            && blit.call_offset == call_offset
            && blit.source_rect == Some(rect)
    })
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_compositor_bounds_every_state_driven_source_call() {
    let Some(audit) = real_audit() else {
        return;
    };
    assert_eq!(audit.blits.len(), 490);
    assert_eq!(audit.source_resolved_count(), 490);
    assert_eq!(audit.source_unresolved_count(), 0);
    assert_eq!(audit.fully_resolved_count(), 444);
    assert_eq!(
        audit
            .blits
            .iter()
            .filter(|blit| blit.source_state_bounded())
            .count(),
        13
    );

    let dynamic_destination = audit
        .blits
        .iter()
        .find(|blit| blit.call_offset == 0x23FD)
        .unwrap();
    assert_eq!(dynamic_destination.resource, Some("OP1.CNS"));
    assert!(dynamic_destination.source_resolved());
    assert!(!dynamic_destination.destination_resolved());

    let state_bounded_source = audit
        .blits
        .iter()
        .find(|blit| blit.call_offset == 0x372D)
        .unwrap();
    assert_eq!(state_bounded_source.resource, Some("OP8.CNS"));
    assert_eq!(state_bounded_source.source_rect, None);
    assert_eq!(
        state_bounded_source.source_envelope,
        Some(Rect {
            x: 320,
            y: 0,
            width: 320,
            height: 360,
        })
    );
    assert!(state_bounded_source.source_resolved());
    assert!(state_bounded_source.destination_resolved());

    assert!(
        !audit
            .blits
            .iter()
            .any(|blit| blit.resource == Some("OP10.CNS"))
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn all_thirteen_state_source_envelopes_match_the_proven_unions() {
    let Some(audit) = real_audit() else {
        return;
    };
    let expected = [
        (
            0x2530,
            "OP3.CNS",
            Rect {
                x: 32,
                y: 264,
                width: 32,
                height: 22,
            },
        ),
        (
            0x254A,
            "OP1.CNS",
            Rect {
                x: 32,
                y: 264,
                width: 32,
                height: 22,
            },
        ),
        (
            0x3589,
            "OP7.CNS",
            Rect {
                x: 320,
                y: 200,
                width: 304,
                height: 200,
            },
        ),
        (
            0x35BC,
            "OP8.CNS",
            Rect {
                x: 0,
                y: 0,
                width: 320,
                height: 200,
            },
        ),
        (
            0x3600,
            "OP8.CNS",
            Rect {
                x: 0,
                y: 0,
                width: 312,
                height: 200,
            },
        ),
        (
            0x372D,
            "OP8.CNS",
            Rect {
                x: 320,
                y: 0,
                width: 320,
                height: 360,
            },
        ),
        (
            0x3778,
            "OP9.CNS",
            Rect {
                x: 320,
                y: 200,
                width: 320,
                height: 200,
            },
        ),
        (
            0x37FD,
            "OP11.CNS",
            Rect {
                x: 0,
                y: 0,
                width: 320,
                height: 336,
            },
        ),
        (
            0x3EF9,
            "OP13.CNS",
            Rect {
                x: 320,
                y: 0,
                width: 320,
                height: 192,
            },
        ),
        (
            0x3F16,
            "OP12.CNS",
            Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 112,
            },
        ),
        (
            0x3F40,
            "OP14.CNS",
            Rect {
                x: 0,
                y: 96,
                width: 320,
                height: 104,
            },
        ),
        (
            0x3F61,
            "OP13.CNS",
            Rect {
                x: 0,
                y: 40,
                width: 64,
                height: 56,
            },
        ),
        (
            0x48BE,
            "OP13.CNS",
            Rect {
                x: 0,
                y: 200,
                width: 128,
                height: 200,
            },
        ),
    ];

    for (call_offset, resource, envelope) in expected {
        let blit = audit
            .blits
            .iter()
            .find(|blit| blit.call_offset == call_offset)
            .unwrap();
        assert_eq!(blit.resource, Some(resource));
        assert_eq!(blit.source_envelope, Some(envelope));
        assert_eq!(blit.source_resolution_label(), "state_bounded_union");
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn op14_uses_the_configured_40_byte_source_stride() {
    let Some(audit) = real_audit() else {
        return;
    };
    let blit = audit
        .blits
        .iter()
        .find(|blit| blit.call_offset == 0x3FED)
        .unwrap();
    assert_eq!(blit.resource, Some("OP14.CNS"));
    assert_eq!(blit.args.source_row_bytes, Some(40));
    assert_eq!(
        blit.source_rect,
        Some(Rect {
            x: 0,
            y: 88,
            width: 320,
            height: 112,
        })
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn baked_latin_candidate_regions_are_statically_sourced() {
    let Some(audit) = real_audit() else {
        return;
    };

    // OP1 ARLE block at the lower-left of the source sheet.
    assert!(has_source_rect(
        &audit,
        "OP1.CNS",
        0x27AC,
        Rect {
            x: 0,
            y: 360,
            width: 96,
            height: 40,
        }
    ));
    // OP2's flower ARLE lies in the two bottom-half source slices.
    assert!(has_source_rect(
        &audit,
        "OP2.CNS",
        0x279B,
        Rect {
            x: 0,
            y: 200,
            width: 144,
            height: 200,
        }
    ));
    assert!(has_source_rect(
        &audit,
        "OP2.CNS",
        0x27BE,
        Rect {
            x: 144,
            y: 200,
            width: 176,
            height: 200,
        }
    ));
    // OP5 contains one RURUE label in each half of the bottom sheet.
    assert!(has_source_rect(
        &audit,
        "OP5.CNS",
        0x3289,
        Rect {
            x: 0,
            y: 200,
            width: 208,
            height: 200,
        }
    ));
    assert!(has_source_rect(
        &audit,
        "OP5.CNS",
        0x3246,
        Rect {
            x: 368,
            y: 200,
            width: 128,
            height: 40,
        }
    ));
    // OP8 SHE-ZO and OP13 CARBUNCLE are inside these sourced slices.
    assert!(has_source_rect(
        &audit,
        "OP8.CNS",
        0x375B,
        Rect {
            x: 0,
            y: 200,
            width: 320,
            height: 200,
        }
    ));
    assert!(has_source_rect(
        &audit,
        "OP13.CNS",
        0x490B,
        Rect {
            x: 128,
            y: 368,
            width: 152,
            height: 32,
        }
    ));
    // OP15's repeated ARS art occurs throughout these large sourced regions.
    assert!(has_source_rect(
        &audit,
        "OP15.CNS",
        0x49E3,
        Rect {
            x: 0,
            y: 192,
            width: 208,
            height: 200,
        }
    ));
    assert!(has_source_rect(
        &audit,
        "OP15.CNS",
        0x4A2F,
        Rect {
            x: 392,
            y: 160,
            width: 112,
            height: 144,
        }
    ));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn op20_is_consumed_as_a_linear_320_by_200_source() {
    let Some(audit) = real_audit() else {
        return;
    };
    let blit = audit
        .blits
        .iter()
        .find(|blit| blit.call_offset == 0x1D63)
        .unwrap();
    assert_eq!(blit.resource, Some("OP20.CNS"));
    assert_eq!(
        blit.source_rect,
        Some(Rect {
            x: 0,
            y: 0,
            width: 320,
            height: 200,
        })
    );
    assert_eq!(
        blit.source_span,
        Some(ByteSpan {
            offset: 0,
            length: 32_000,
            row_bytes: 40,
            rows: 200,
            planes: 4,
        })
    );
    assert_eq!(
        blit.destination_rect,
        Some(Rect {
            x: 160,
            y: 96,
            width: 320,
            height: 200,
        })
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_profile_rejects_a_drifted_filename_anchor() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL").unwrap();
    let mut decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .unwrap()
        .output;
    decoded[0x0210 - 0x0100] = b'x';
    let error = audit_demo_compositor(&decoded).unwrap_err().to_string();
    assert!(error.contains("profile mismatch at filename logical offset 0x0210"));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_profile_rejects_a_drifted_buffer_anchor() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL").unwrap();
    let mut decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .unwrap()
        .output;
    decoded[0x03C8 - 0x0100] ^= 1;
    let error = audit_demo_compositor(&decoded).unwrap_err().to_string();
    assert!(error.contains("OP1.CNS buffer anchor logical offset 0x03C8"));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_profile_rejects_a_drifted_state_envelope_anchor() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL").unwrap();
    let mut decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .unwrap()
        .output;
    decoded[0x1915 - 0x0100] ^= 1;
    let error = audit_demo_compositor(&decoded).unwrap_err().to_string();
    assert!(error.contains("state profile mismatch at logical offset 0x1915"));
}
