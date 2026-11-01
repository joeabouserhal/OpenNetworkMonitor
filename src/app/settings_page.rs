use super::*;
use chrono::TimeZone as _;
use eframe::egui::{Align, Align2, Layout, Margin, Pos2, Rect, Sense, Shape, StrokeKind};
use widgets::{
    ButtonKind, bold, divider, paint_text, pal, radius, sans, setting_row, strong_text, thin,
};

/// A titled group of rows inside one card.
fn section(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(22.0);
    ui.label(RichText::new(title).font(bold(13.0)).color(pal(ui).dim));
    ui.add_space(8.0);
    widgets::surface(ui)
        .inner_margin(Margin::symmetric(22, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(ui);
        });
}

/// How much of this month's allowance is used, with the date it began.
fn cap_meter(ui: &mut egui::Ui, cap: crate::worker::CapStatus) {
    let p = pal(ui);
    let share = (cap.used as f64 / cap.limit.max(1) as f64).clamp(0.0, 1.0) as f32;
    let tone = match crate::alerts::cap_level(cap.used, cap.limit) {
        crate::alerts::CapLevel::Below => p.accent,
        crate::alerts::CapLevel::Warning => p.warn,
        crate::alerts::CapLevel::Reached => p.bad,
    };
    let since = chrono::Local
        .timestamp_opt(cap.cycle_start, 0)
        .single()
        .map(|time| time.format("%d %b").to_string())
        .unwrap_or_default();
    ui.label(
        RichText::new(format!(
            "{} of {} used since {since} ({:.0}%)",
            format_bytes(cap.used as f64),
            format_bytes(cap.limit as f64),
            share * 100.0
        ))
        .size(13.5)
        .color(p.text),
    );
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    ui.painter().rect_filled(rect, radius(4.0), p.raised);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width() * share, rect.height())),
        radius(4.0),
        tone,
    );
}

/// Preview tiles that show each palette's canvas, card and data colours.
fn theme_picker(ui: &mut egui::Ui, current: &mut Theme) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let gap = 12.0;
    let tile = ((ui.available_width() - gap * 3.0) / 4.0)
        .floor()
        .min(180.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for theme in Theme::ALL {
            let swatch = theme.palette();
            let (rect, response) = ui.allocate_exact_size(Vec2::new(tile, 96.0), Sense::click());
            let on = *current == theme;
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    on,
                    theme.label(),
                )
            });
            let preview = Rect::from_min_size(rect.min, Vec2::new(tile, 68.0));
            let painter = ui.painter();
            painter.rect_filled(preview, radius(11.0), swatch.canvas);
            let card = preview.shrink2(Vec2::new(10.0, 10.0));
            painter.rect_filled(card, radius(7.0), swatch.card);
            painter.rect_stroke(
                card,
                radius(7.0),
                widgets::px_stroke(ui.ctx(), 1.0, swatch.hairline),
                StrokeKind::Inside,
            );
            painter.circle_filled(card.left_top() + Vec2::new(9.0, 9.0), 3.0, swatch.accent);
            painter.rect_filled(
                Rect::from_min_size(
                    card.left_top() + Vec2::new(18.0, 7.0),
                    Vec2::new(card.width() * 0.3, 4.0),
                ),
                radius(2.0),
                swatch.dim.gamma_multiply(0.7),
            );
            let base = card.bottom() - 7.0;
            let step = (card.width() - 16.0) / 6.0;
            for (color, heights) in [
                (swatch.down, [4.0, 12.0, 7.0, 15.0, 9.0, 12.0, 6.0]),
                (swatch.up, [2.0, 5.0, 3.0, 6.0, 3.0, 4.0, 2.0]),
            ] {
                let points = heights
                    .iter()
                    .enumerate()
                    .map(|(i, h)| Pos2::new(card.left() + 8.0 + i as f32 * step, base - h))
                    .collect();
                painter.add(Shape::line(points, widgets::snapped(ui.ctx(), 1.6, color)));
            }
            painter.rect_stroke(
                preview,
                radius(11.0),
                if on {
                    widgets::px_stroke(ui.ctx(), 2.0, p.accent)
                } else {
                    widgets::px_stroke(ui.ctx(), 1.0, p.hairline.gamma_multiply(2.0))
                },
                StrokeKind::Inside,
            );
            paint_text(
                painter,
                Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 72.0), rect.max),
                Align2::LEFT_CENTER,
                theme.label(),
                if on { bold(13.5) } else { sans(13.5) },
                if on { p.text } else { p.dim },
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !on
            {
                *current = theme;
                changed = true;
            }
        }
    });
    changed
}

impl MonitorApp {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        section(ui, "Appearance", |ui| {
            ui.add_space(14.0);
            ui.label(strong_text("Theme", 15.0));
            ui.add_space(2.0);
            ui.label(RichText::new("Choose a palette.").size(13.5).weak());
            ui.add_space(12.0);
            let mut shown = self.settings.active_theme(ui.ctx());
            if theme_picker(ui, &mut shown) {
                // Choosing a palette by hand ends following Windows.
                self.settings.theme = shown;
                self.settings.follow_system = false;
                shown.apply(ui.ctx());
                self.applied_theme = shown;
            }
            ui.add_space(16.0);
            divider(ui);
            setting_row(
                ui,
                "Match Windows appearance",
                "Switches between Slate and Light with the Windows light or dark setting.",
                56.0,
                |ui| {
                    widgets::toggle(
                        ui,
                        &mut self.settings.follow_system,
                        "Match Windows appearance",
                    );
                },
            );
            divider(ui);
            let before = self.settings.ui_scale;
            setting_row(
                ui,
                "Interface size",
                "Scales text and controls.",
                300.0,
                |ui| {
                    if widgets::button(ui, ButtonKind::Ghost, "Reset").clicked() {
                        self.settings.ui_scale = 0.9;
                    }
                    ui.label(
                        RichText::new(format!("{:.0}%", self.settings.ui_scale * 100.0))
                            .font(bold(14.0)),
                    );
                    widgets::slider(
                        ui,
                        &mut self.settings.ui_scale,
                        0.8..=1.1,
                        0.05,
                        170.0,
                        "Interface size",
                    );
                },
            );
            if before != self.settings.ui_scale {
                ui.ctx().set_zoom_factor(self.settings.ui_scale);
            }
            divider(ui);
            setting_row(
                ui,
                "Live speed units",
                "Usage totals and CSV exports always use bytes.",
                220.0,
                |ui| {
                    let mut bits = self.settings.bits;
                    if widgets::segmented(
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
        section(ui, "Background monitoring", |ui| {
            self.startup.poll();
            let mut enabled = self.startup.enabled;
            // An unprotected copy may still turn startup off, never on.
            let usable = self.startup.available
                && !self.startup.busy
                && (self.startup.enabled || self.startup.location_risk.is_none());
            setting_row(
                ui,
                "Launch when I sign in to Windows",
                "Starts in the tray and monitors in the background.",
                56.0,
                |ui| {
                    ui.add_enabled_ui(usable, |ui| {
                        if widgets::toggle(ui, &mut enabled, "Launch when I sign in to Windows")
                            .changed()
                        {
                            self.startup.set_enabled(enabled);
                        }
                    });
                },
            );
            if self.startup.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Updating startup settings…")
                            .size(13.5)
                            .weak(),
                    );
                });
                ui.add_space(10.0);
            }
            if let Some(risk) = &self.startup.location_risk {
                let advice = if self.startup.enabled {
                    "Startup runs with administrator rights, so anything that can change this copy could run elevated. Turn it off, or move the app to Program Files."
                } else {
                    "Startup runs with administrator rights, so the app has to live in a protected folder such as Program Files."
                };
                widgets::message(ui, &format!("{advice}\n{risk}"), true, false);
                ui.add_space(10.0);
            }
            if self.startup.enabled && self.startup.different_copy {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Startup points to another copy of the app.")
                            .size(13.5)
                            .weak(),
                    );
                    ui.add_enabled_ui(!self.startup.busy, |ui| {
                        if widgets::button(ui, ButtonKind::Secondary, "Use this copy").clicked() {
                            self.startup.set_enabled(true);
                        }
                    });
                });
                ui.add_space(10.0);
            }
            if !self.startup.error.is_empty() {
                ui.add_space(6.0);
                widgets::message(ui, &self.startup.error, true, false);
            }
            divider(ui);
            let has_tray = self.tray.is_some();
            setting_row(
                ui,
                "Close window to system tray",
                "Click the tray icon to reopen. Tray → Exit stops monitoring immediately.",
                56.0,
                |ui| {
                    ui.add_enabled_ui(has_tray, |ui| {
                        widgets::toggle(
                            ui,
                            &mut self.settings.close_to_tray,
                            "Close window to system tray",
                        );
                    });
                },
            );
        });
        section(ui, "Monitoring", |ui| {
            let active = self.data.snapshot.etw_active;
            setting_row(
                ui,
                "Capture",
                "Built-in Windows capture. Administrator access is requested on launch.",
                300.0,
                |ui| {
                    // Nothing to retry while detailed capture is already running.
                    ui.add_enabled_ui(!active, |ui| {
                        if widgets::button(ui, ButtonKind::Secondary, "Retry capture").clicked() {
                            let _ = self.worker.tx.send(Command::RetryCapture);
                        }
                    });
                    widgets::badge(
                        ui,
                        if active {
                            "Detailed capture active"
                        } else {
                            "Detailed capture unavailable"
                        },
                        if active { p.good } else { p.warn },
                    );
                },
            );
            if !active {
                ui.label(
                    RichText::new(&self.data.snapshot.capture_status)
                        .size(13.5)
                        .weak(),
                );
                ui.add_space(10.0);
            }
            divider(ui);
            ui.add_space(14.0);
            ui.columns(4, |cols| {
                for (col, (label, value)) in cols.iter_mut().zip([
                    ("Events", self.data.snapshot.parsed_events),
                    ("Decode errors", self.data.snapshot.parse_errors),
                    ("Queue drops", self.data.snapshot.queue_drops),
                    ("Kernel losses", self.data.snapshot.kernel_losses),
                ]) {
                    col.label(RichText::new(label).size(13.0).weak());
                    col.label(
                        RichText::new(format_count(value))
                            .font(thin(26.0))
                            .color(p.text),
                    );
                }
            });
            ui.add_space(14.0);
        });
        section(ui, "IP countries", |ui| {
            let selected = self
                .settings
                .geo
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "No database selected".into());
            ui.add_space(14.0);
            ui.label(strong_text("Offline lookup", 15.0));
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new("Optional GeoLite2 / GeoIP2 .mmdb database. Locations are approximate and can represent a VPN exit or hosting provider.")
                        .size(13.5)
                        .weak(),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    RichText::new(selected.clone())
                        .font(widgets::mono(13.0))
                        .color(p.dim),
                )
                .truncate(),
            )
            .on_hover_text(selected);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_enabled_ui(!self.dialogs.busy(), |ui| {
                    if widgets::button(ui, ButtonKind::Secondary, "Choose database…").clicked() {
                        self.dialogs.pick_country_database(ui.ctx());
                    }
                });
                if self.settings.geo.is_some()
                    && widgets::button(ui, ButtonKind::Ghost, "Disable lookup").clicked()
                {
                    self.settings.geo = None;
                    let _ = self.worker.tx.send(Command::Geo(None));
                }
            });
            ui.add_space(14.0);
        });
        section(ui, "Alerts", |ui| {
            setting_row(
                ui,
                "New app on the network",
                "Notify the first time an app is seen using the network.",
                56.0,
                |ui| {
                    widgets::toggle(
                        ui,
                        &mut self.settings.alert_new_apps,
                        "New app on the network",
                    );
                },
            );
            divider(ui);
            setting_row(
                ui,
                "Monthly data cap",
                "Notify at 80% and when the allowance is used up. Usage counts all adapters.",
                56.0,
                |ui| {
                    widgets::toggle(ui, &mut self.settings.cap_enabled, "Monthly data cap");
                },
            );
            if self.settings.cap_enabled {
                divider(ui);
                setting_row(ui, "Allowance", "In GiB for each month.", 150.0, |ui| {
                    ui.label(RichText::new("GiB").size(13.5).weak());
                    let edit = period::input_frame(ui)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cap_input)
                                    .font(widgets::mono(14.0))
                                    .char_limit(7)
                                    .desired_width(64.0)
                                    .frame(egui::Frame::NONE),
                            )
                        })
                        .inner;
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
                divider(ui);
                let mut day = f32::from(self.settings.cap_reset_day);
                setting_row(
                    ui,
                    "Starts over on day",
                    "The day of the month the allowance resets.",
                    260.0,
                    |ui| {
                        ui.label(RichText::new(format!("{day:.0}")).font(bold(14.0)));
                        widgets::slider(ui, &mut day, 1.0..=28.0, 1.0, 190.0, "Reset day");
                    },
                );
                self.settings.cap_reset_day = day.round().clamp(1.0, 28.0) as u8;
                if let Some(cap) = self.data.cap {
                    ui.add_space(4.0);
                    cap_meter(ui, cap);
                    ui.add_space(14.0);
                }
            }
        });
        section(ui, "History & exports", |ui| {
            ui.add_space(14.0);
            ui.label(strong_text("Saved usage", 15.0));
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Usage history is kept indefinitely. CSV follows the current Overview filters; timestamps use UTC.",
                )
                .size(13.5)
                .weak(),
            );
            ui.add_space(12.0);
            let folder = self.data_dir.display().to_string();
            ui.add(
                egui::Label::new(
                    RichText::new(folder.clone())
                        .font(widgets::mono(13.0))
                        .color(p.dim),
                )
                .truncate(),
            )
            .on_hover_text(folder);
            ui.add_space(12.0);
            if widgets::button_with(
                ui,
                ButtonKind::Secondary,
                Some(widgets::Icon::Export),
                "Export selected usage to CSV…",
                None,
            )
            .clicked()
            {
                self.export(ui.ctx());
            }
            ui.add_space(14.0);
        });
        section(ui, "About", |ui| {
            egui::CollapsingHeader::new(strong_text("How measurements work", 15.0)).show(ui, |ui| {
                for text in [
                    "Adapters count interface bytes. App traffic counts observed TCP/UDP transfers. These views measure different layers and are never added together.",
                    "LAN includes private, loopback, link-local, multicast and on-link addresses. WAN covers public destinations; reserved/shared addresses are Other.",
                    "App traffic is assigned by local IP. Ambiguous matches stay Unassigned. VPN traffic can appear at multiple layers, so All adapters is a sum of observations rather than a guaranteed unique internet total.",
                    "Traffic types are inferred from ports. Host names use Windows reverse DNS and may identify shared infrastructure rather than the visited website.",
                    "Monitoring continues while this desktop app is running. A Windows service is not included yet.",
                ] {
                    ui.add(egui::Label::new(RichText::new(text).size(14.0).weak()).wrap());
                    ui.add_space(6.0);
                }
            });
            ui.add_space(10.0);
        });
        ui.add_space(18.0);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.label(
                RichText::new(concat!(
                    "OpenNetworkMonitor ",
                    env!("CARGO_PKG_VERSION"),
                    " · MIT · Rust + egui"
                ))
                .size(13.0)
                .weak(),
            );
        });
        ui.add_space(24.0);
    }
}
