//! Settings: stacked cards of label-and-control rows.
use super::*;
use crate::ui::{
    controls::{self, AlertVariant, BadgeVariant},
    layout::{card, tabs},
    tokens::{BaseColor, Tokens, hairline},
};
use chrono::TimeZone as _;

/// A titled card of settings rows.
fn section(ui: &mut egui::Ui, title: &str, description: &str, rows: impl FnOnce(&mut egui::Ui)) {
    card(ui, 0, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::new()
            .inner_margin(Margin {
                left: 20,
                right: 20,
                top: 18,
                bottom: 14,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                layout::card_header(ui, title, description, |_| {});
            });
        rows(ui);
    });
}

/// One row: title and description on the left, `control` on the right, with a border above.
fn row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui),
) {
    let t = tk(ui.ctx());
    let width = ui.available_width();
    let text_width = (width - control_width - 60.0).max(160.0);
    let painter = ui.painter().clone();
    let title_galley = painter.layout(
        title.to_owned(),
        text::medium(14.0),
        t.foreground,
        text_width,
    );
    let description_galley = (!description.is_empty()).then(|| {
        painter.layout(
            description.to_owned(),
            text::regular(13.0),
            t.muted_foreground,
            text_width,
        )
    });
    let text_height = title_galley.size().y
        + description_galley
            .as_ref()
            .map_or(0.0, |g| g.size().y + 2.0);
    let height = (text_height + 28.0).max(60.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            format!("{title}. {description}"),
        )
    });
    painter.hline(rect.x_range(), rect.top(), hairline(ui.ctx(), t.border));
    let top = rect.center().y - text_height / 2.0;
    painter.galley(
        Pos2::new(rect.left() + 20.0, top),
        title_galley.clone(),
        t.foreground,
    );
    if let Some(galley) = description_galley {
        painter.galley(
            Pos2::new(rect.left() + 20.0, top + title_galley.size().y + 2.0),
            galley,
            t.muted_foreground,
        );
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - 20.0 - control_width, rect.top()),
                Pos2::new(rect.right() - 20.0, rect.bottom()),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 8.0;
    control(&mut child);
}

/// A full-width block inside a section, with a border above.
fn block(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let t = tk(ui.ctx());
    let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        line.x_range(),
        line.center().y,
        hairline(ui.ctx(), t.border),
    );
    egui::Frame::new()
        .inner_margin(Margin::symmetric(20, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            content(ui);
        });
}

fn note(ui: &mut egui::Ui, message: &str) {
    let t = tk(ui.ctx());
    let galley = ui.painter().layout(
        message.to_owned(),
        text::regular(13.5),
        t.muted_foreground,
        ui.available_width(),
    );
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, t.muted_foreground);
}

fn path_text(ui: &mut egui::Ui, path: &str) {
    let t = tk(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    text::paint(
        ui.painter(),
        rect,
        Align2::LEFT_CENTER,
        path,
        text::mono(12.5),
        t.muted_foreground,
    );
    response.on_hover_text(path);
}

impl MonitorApp {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        // The form is long, so it scrolls under the page header.
        egui::ScrollArea::vertical()
            .id_salt("settings-form")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                self.appearance_section(ui);
                self.background_section(ui);
                self.monitoring_section(ui);
                self.countries_section(ui);
                self.alerts_section(ui);
                self.history_section(ui);
                self.about_section(ui);
                ui.add_space(20.0);
            });
    }

    fn appearance_section(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        section(ui, "Appearance", "How the app looks.", |ui| {
            row(
                ui,
                "Theme",
                "Follow Windows, or always use light or dark.",
                260.0,
                |ui| {
                    let mut appearance = self.settings.appearance();
                    if tabs(
                        ui,
                        "appearance",
                        &mut appearance,
                        &[
                            (Appearance::Light, "Light"),
                            (Appearance::Dark, "Dark"),
                            (Appearance::System, "System"),
                        ],
                    ) {
                        self.settings.appearance = Some(appearance);
                    }
                },
            );
            block(ui, |ui| {
                let (label, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                text::paint(
                    ui.painter(),
                    label,
                    Align2::LEFT_CENTER,
                    "Accent colour",
                    text::medium(14.0),
                    t.foreground,
                );
                note(
                    ui,
                    "Used for primary buttons, focus rings and the download series.",
                );
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    let scheme = self.settings.scheme(ui.ctx());
                    for base in BaseColor::ALL {
                        if swatch(ui, &t, base, scheme, self.settings.base_color == base).clicked()
                        {
                            self.settings.base_color = base;
                        }
                    }
                });
            });
            let before = self.settings.ui_scale;
            row(
                ui,
                "Interface size",
                "Scales text and controls.",
                330.0,
                |ui| {
                    if Button::new("Reset")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .show(ui)
                        .clicked()
                    {
                        self.settings.ui_scale = DEFAULT_UI_SCALE;
                    }
                    let (value, _) = ui.allocate_exact_size(Vec2::new(44.0, 20.0), Sense::hover());
                    text::paint(
                        ui.painter(),
                        value,
                        Align2::RIGHT_CENTER,
                        &format!("{:.0}%", self.settings.ui_scale * 100.0),
                        text::medium(14.0),
                        t.foreground,
                    );
                    controls::slider(
                        ui,
                        &mut self.settings.ui_scale,
                        0.8..=1.1,
                        0.05,
                        190.0,
                        "Interface size",
                    );
                },
            );
            if before != self.settings.ui_scale {
                ui.ctx().set_zoom_factor(self.settings.ui_scale);
            }
            row(
                ui,
                "Live speed units",
                "Usage totals and CSV exports always use bytes.",
                200.0,
                |ui| {
                    let mut bits = self.settings.bits;
                    if tabs(
                        ui,
                        "speed-units",
                        &mut bits,
                        &[(false, "Bytes/s"), (true, "Bits/s")],
                    ) {
                        self.settings.bits = bits;
                    }
                },
            );
        });
    }

    fn background_section(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "Background monitoring",
            "Keep recording when the window is closed.",
            |ui| {
                self.startup.poll();
                let mut enabled = self.startup.enabled;
                // An unprotected copy may still turn startup off, never on.
                let usable = self.startup.available
                    && !self.startup.busy
                    && (self.startup.enabled || self.startup.location_risk.is_none());
                row(
                    ui,
                    "Launch when I sign in to Windows",
                    "Starts in the tray and monitors in the background.",
                    56.0,
                    |ui| {
                        ui.add_enabled_ui(usable, |ui| {
                            if controls::switch(
                                ui,
                                &mut enabled,
                                "Launch when I sign in to Windows",
                            )
                            .changed()
                            {
                                self.startup.set_enabled(enabled);
                            }
                        });
                    },
                );
                if self.startup.busy {
                    block(ui, |ui| note(ui, "Updating startup settings…"));
                }
                if let Some(risk) = self.startup.location_risk.clone() {
                    let advice = if self.startup.enabled {
                        "Startup runs with administrator rights, so anything that can change this copy could run elevated. Turn it off, or move the app to Program Files."
                    } else {
                        "Startup runs with administrator rights, so the app has to live in a protected folder such as Program Files."
                    };
                    block(ui, |ui| {
                        controls::alert(
                            ui,
                            AlertVariant::Destructive,
                            Icon::Alert,
                            "This copy is in an unprotected folder",
                            &format!("{advice} {risk}"),
                            false,
                        );
                    });
                }
                if self.startup.enabled && self.startup.different_copy {
                    let busy = self.startup.busy;
                    row(
                        ui,
                        "Startup points to another copy",
                        "Use this copy so it starts at sign-in instead.",
                        130.0,
                        |ui| {
                            if Button::new("Use this copy")
                                .variant(Variant::Outline)
                                .enabled(!busy)
                                .show(ui)
                                .clicked()
                            {
                                self.startup.set_enabled(true);
                            }
                        },
                    );
                }
                if !self.startup.error.is_empty() {
                    let message = self.startup.error.clone();
                    block(ui, |ui| {
                        controls::alert(
                            ui,
                            AlertVariant::Destructive,
                            Icon::Alert,
                            "Startup could not be changed",
                            &message,
                            false,
                        );
                    });
                }
                let has_tray = self.tray.is_some();
                row(
                    ui,
                    "Close window to system tray",
                    "Click the tray icon to reopen. Tray → Exit stops monitoring immediately.",
                    56.0,
                    |ui| {
                        ui.add_enabled_ui(has_tray, |ui| {
                            controls::switch(
                                ui,
                                &mut self.settings.close_to_tray,
                                "Close window to system tray",
                            );
                        });
                    },
                );
            },
        );
    }

    fn monitoring_section(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        section(
            ui,
            "Monitoring",
            "Built-in Windows capture of app traffic.",
            |ui| {
                let active = self.data.snapshot.etw_active;
                row(
                    ui,
                    "Capture",
                    "Administrator access is requested on launch.",
                    330.0,
                    |ui| {
                        // Nothing to retry while detailed capture is already running.
                        if Button::new("Retry capture")
                            .variant(Variant::Outline)
                            .enabled(!active)
                            .show(ui)
                            .clicked()
                        {
                            let _ = self.worker.tx.send(Command::RetryCapture);
                        }
                        controls::badge(
                            ui,
                            if active {
                                BadgeVariant::Success
                            } else {
                                BadgeVariant::Destructive
                            },
                            if active {
                                "Detailed capture active"
                            } else {
                                "Detailed capture unavailable"
                            },
                        );
                    },
                );
                if !active {
                    let status = self.data.snapshot.capture_status.clone();
                    block(ui, |ui| note(ui, &status));
                }
                block(ui, |ui| {
                    let width = ui.available_width();
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 54.0), Sense::hover());
                    let cell = rect.width() / 4.0;
                    for (i, (label, value)) in [
                        ("Events", self.data.snapshot.parsed_events),
                        ("Decode errors", self.data.snapshot.parse_errors),
                        ("Queue drops", self.data.snapshot.queue_drops),
                        ("Kernel losses", self.data.snapshot.kernel_losses),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let area = Rect::from_min_size(
                            rect.min + Vec2::new(i as f32 * cell, 0.0),
                            Vec2::new(cell - 8.0, rect.height()),
                        );
                        text::paint(
                            ui.painter(),
                            Rect::from_min_size(area.min, Vec2::new(area.width(), 18.0)),
                            Align2::LEFT_CENTER,
                            label,
                            text::regular(13.0),
                            t.muted_foreground,
                        );
                        text::paint(
                            ui.painter(),
                            Rect::from_min_max(area.min + Vec2::new(0.0, 20.0), area.max),
                            Align2::LEFT_CENTER,
                            &format_count(value),
                            text::semibold(22.0),
                            t.foreground,
                        );
                        crate::ui::a11y::label(
                            ui,
                            area,
                            &format!("{label}: {}", format_count(value)),
                        );
                    }
                });
            },
        );
    }

    fn countries_section(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "IP countries",
            "Optional offline country lookup.",
            |ui| {
                block(ui, |ui| {
                    note(
                        ui,
                        "Provide a GeoLite2 or GeoIP2 .mmdb database. Locations are approximate and can represent a VPN exit or hosting provider. No online lookups are made.",
                    );
                    let selected = self
                        .settings
                        .geo
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "No database selected".into());
                    path_text(ui, &selected);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let busy = self.dialogs.busy();
                        if Button::new("Choose database…")
                            .variant(Variant::Outline)
                            .enabled(!busy)
                            .show(ui)
                            .clicked()
                        {
                            self.dialogs.pick_country_database(ui.ctx());
                        }
                        if self.settings.geo.is_some()
                            && Button::new("Disable lookup")
                                .variant(Variant::Ghost)
                                .show(ui)
                                .clicked()
                        {
                            self.settings.geo = None;
                            let _ = self.worker.tx.send(Command::Geo(None));
                        }
                    });
                });
            },
        );
    }

    fn alerts_section(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        section(
            ui,
            "Alerts",
            "Windows notifications for unusual or heavy use.",
            |ui| {
                row(
                    ui,
                    "New app on the network",
                    "Notify the first time an app is seen using the network.",
                    56.0,
                    |ui| {
                        controls::switch(
                            ui,
                            &mut self.settings.alert_new_apps,
                            "New app on the network",
                        );
                    },
                );
                row(
                    ui,
                    "Monthly data cap",
                    "Notify at 80% and when the allowance is used up. Usage counts all adapters.",
                    56.0,
                    |ui| {
                        controls::switch(ui, &mut self.settings.cap_enabled, "Monthly data cap");
                    },
                );
                if self.settings.cap_enabled {
                    row(ui, "Allowance", "In GiB for each month.", 150.0, |ui| {
                        let (unit, _) =
                            ui.allocate_exact_size(Vec2::new(28.0, 20.0), Sense::hover());
                        text::paint(
                            ui.painter(),
                            unit,
                            Align2::LEFT_CENTER,
                            "GiB",
                            text::regular(13.5),
                            t.muted_foreground,
                        );
                        let edit = controls::input(ui, &mut self.cap_input, "100", 96.0);
                        if edit.changed()
                            && let Ok(value) = self.cap_input.trim().parse::<f32>()
                            && value.is_finite()
                            && value > 0.0
                        {
                            self.settings.cap_gib = value.min(1_000_000.0);
                        }
                        if edit.lost_focus() {
                            self.cap_input = format_allowance(self.settings.cap_gib);
                        }
                    });
                    let mut day = f32::from(self.settings.cap_reset_day);
                    row(
                        ui,
                        "Starts over on day",
                        "The day of the month the allowance resets.",
                        270.0,
                        |ui| {
                            let (value, _) =
                                ui.allocate_exact_size(Vec2::new(30.0, 20.0), Sense::hover());
                            text::paint(
                                ui.painter(),
                                value,
                                Align2::RIGHT_CENTER,
                                &format!("{day:.0}"),
                                text::medium(14.0),
                                t.foreground,
                            );
                            controls::slider(ui, &mut day, 1.0..=28.0, 1.0, 200.0, "Reset day");
                        },
                    );
                    self.settings.cap_reset_day = day.round().clamp(1.0, 28.0) as u8;
                    if let Some(cap) = self.data.cap {
                        block(ui, |ui| {
                            let since = chrono::Local
                                .timestamp_opt(cap.cycle_start, 0)
                                .single()
                                .map(|time| time.format("%d %b").to_string())
                                .unwrap_or_default();
                            let share = cap.used as f64 / cap.limit.max(1) as f64;
                            let tone = match crate::alerts::cap_level(cap.used, cap.limit) {
                                crate::alerts::CapLevel::Below => None,
                                crate::alerts::CapLevel::Warning => Some(t.warning),
                                crate::alerts::CapLevel::Reached => Some(t.destructive),
                            };
                            note(
                                ui,
                                &format!(
                                    "{} of {} used since {since} ({:.0}%)",
                                    format_bytes(cap.used as f64),
                                    format_bytes(cap.limit as f64),
                                    share * 100.0
                                ),
                            );
                            controls::progress(ui, share as f32, tone);
                        });
                    }
                }
            },
        );
    }

    fn history_section(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "History and exports",
            "Usage history is kept until you delete it.",
            |ui| {
                block(ui, |ui| {
                    note(
                        ui,
                        "CSV follows the current Overview filters. Timestamps use UTC and byte counts are raw.",
                    );
                    path_text(ui, &self.data_dir.display().to_string());
                    if Button::new("Export selected usage to CSV…")
                        .icon(Icon::Download)
                        .variant(Variant::Outline)
                        .show(ui)
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.export(&ctx);
                    }
                });
            },
        );
    }

    fn about_section(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "About",
            concat!(
                "OpenNetworkMonitor ",
                env!("CARGO_PKG_VERSION"),
                " · MIT · Rust and egui"
            ),
            |ui| {
                block(ui, |ui| {
                    for paragraph in [
                        "Adapters count interface bytes. App traffic counts observed TCP/UDP transfers. These views measure different layers and are never added together.",
                        "LAN includes private, loopback, link-local, multicast and on-link addresses. WAN covers public destinations; reserved or shared addresses are Other.",
                        "App traffic is assigned by local IP. Ambiguous matches stay Unassigned. VPN traffic can appear at several layers, so All adapters is a sum of observations rather than a unique internet total.",
                        "Traffic types are inferred from ports. Host names use Windows reverse DNS and may name shared infrastructure rather than the website visited.",
                        "Monitoring continues while this desktop app is running. A Windows service is not included yet.",
                    ] {
                        note(ui, paragraph);
                    }
                });
            },
        );
    }
}

/// A round colour choice for the accent.
fn swatch(
    ui: &mut egui::Ui,
    t: &Tokens,
    base: BaseColor,
    scheme: crate::ui::tokens::Scheme,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, base.label())
    });
    let painter = ui.painter();
    if selected || response.hovered() {
        painter.circle_stroke(
            rect.center(),
            15.0,
            egui::Stroke::new(2.0, if selected { t.foreground } else { t.border }),
        );
    }
    painter.circle_filled(rect.center(), 11.0, base.primary(scheme));
    if selected {
        let on = Tokens::new(scheme, base).primary_foreground;
        icon::paint(painter, Icon::Check, rect.center(), 13.0, on);
    }
    response
        .on_hover_text(base.label())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
