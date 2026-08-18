#![windows_subsystem = "windows"]

use anyhow::{anyhow, bail, Context, Result};
use image::Luma;
use native_windows_gui as nwg;
use qrcode::QrCode;
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use tempfile::TempDir;
use umya_spreadsheet::structs::{drawing::spreadsheet::MarkerType, Image};

const TABLE: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz+/";
const KEY: [usize; 5] = [2, 4, 8, 16, 32];
const TEMPLATE_BYTES: &[u8] = include_bytes!("../assets/模板.xlsx");

fn encode_label(input: &str) -> Result<String> {
    let mut out = String::with_capacity(input.len() + 2);
    out.push_str("A#");
    let table = TABLE.as_bytes();

    for (i, b) in input.as_bytes().iter().copied().enumerate() {
        let pos = table
            .iter()
            .position(|v| *v == b)
            .ok_or_else(|| anyhow!("编码中包含不支持的字符：{}", char::from(b)))?;
        out.push(table[(pos + KEY[i % KEY.len()]) % 64] as char);
    }
    Ok(out)
}

fn ensure_xlsx_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("xlsx");
    }
    path
}

fn make_qr_png(data: &str, path: &Path) -> Result<()> {
    let code = QrCode::new(data.as_bytes()).context("生成二维码失败")?;
    let image = code
        .render::<Luma<u8>>()
        .quiet_zone(true)
        .min_dimensions(96, 96)
        .max_dimensions(96, 96)
        .build();
    image.save(path).context("保存二维码临时图片失败")?;
    Ok(())
}

fn export_workbook(input: &Path, output: &Path) -> Result<usize> {
    let mut book = umya_spreadsheet::reader::xlsx::read(input)
        .with_context(|| format!("无法读取 Excel：{}", input.display()))?;

    let sheet = book
        .get_sheet_by_name_mut("Sheet1")
        .ok_or_else(|| anyhow!("模板中未找到 Sheet1 工作表"))?;

    // 用户提供的模板：A=序号，B=标签编码，C=二维码。
    let header = sheet.get_value((2, 1));
    if header.trim() != "标签编码" {
        bail!("模板格式不匹配：Sheet1 的 B1 应为“标签编码”");
    }

    // 若导入的是已经生成过的文件，避免重复叠加二维码图片。
    sheet.get_image_collection_mut().clear();
    sheet.get_column_dimension_mut("C").set_width(15.0);

    let max_row = sheet.get_highest_row();
    let temp = TempDir::new().context("创建临时目录失败")?;
    let mut count = 0usize;

    for row in 2..=max_row {
        let raw = sheet.get_value((2, row));
        let label = raw.trim();
        if label.is_empty() {
            continue;
        }

        let encrypted = encode_label(label)
            .with_context(|| format!("第 {row} 行标签编码无效：{label}"))?;
        let qr_path = temp.path().join(format!("qr_{row}.png"));
        make_qr_png(&encrypted, &qr_path)?;

        let mut marker = MarkerType::default();
        marker.set_coordinate(format!("C{row}"));

        let mut qr_image = Image::default();
        let qr_path_string = qr_path.to_string_lossy().into_owned();
        qr_image.new_image(&qr_path_string, marker);
        sheet.add_image(qr_image);
        sheet.get_row_dimension_mut(&row).set_height(78.0);
        count += 1;
    }

    if count == 0 {
        bail!("没有找到可生成二维码的标签编码，请在 Sheet1 的 B 列第 2 行开始填写编码");
    }

    umya_spreadsheet::writer::xlsx::write(&book, output)
        .with_context(|| format!("导出 Excel 失败：{}", output.display()))?;
    Ok(count)
}

fn main() {
    nwg::init().expect("初始化 Windows GUI 失败");
    let _ = nwg::Font::set_global_family("Microsoft YaHei UI")
        .or_else(|_| nwg::Font::set_global_family("Segoe UI"));

    let mut window = nwg::Window::default();
    let mut import_button = nwg::Button::default();
    let mut export_button = nwg::Button::default();
    let mut template_button = nwg::Button::default();
    layout = nwg::GridLayout::default();

    let mut import_dialog = nwg::FileDialog::default();
    let mut export_dialog = nwg::FileDialog::default();
    let mut template_dialog = nwg::FileDialog::default();

    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .size((390, 135))
        .position((420, 320))
        .title("二维码批量生成器")
        .build(&mut window)
        .expect("创建窗#�W失败");

    nwg::Button::builder()
        .text("导入")
        .parent(&window)
        .build(&mut import_button)
        .expect("创建导入按钮失败");

    nwg::Button::builder()
        .text("导出")
        .parent(&window)
        .build(&mut export_button)
        .expect("创建导出按钮失败");

    nwg::Button::builder()
        .text("模板下载")
        .parent(&window)
        .build(&mut template_button)
        .expect("创建模板按钮失败");

    nwg::GridLayout::builder()
        .parent(&window)
        .spacing(8)
        .margin([18, 28, 18, 28])
        .child(0, 0, &import_button)
        .child(1, 0, &export_button)
        .child(2, 0, &template_button)
        .build(&layout)
        .expect("创建布局失败");

    nwg::FileDialog::builder()
        .title("导入 Excel")
        .action(nwg::FileDialogAction::Open)
        .filters("Excel 工作簿(*.xlsx)")
        .build(&mut import_dialog)
        .expect("创建导入对话框失败");

    nwg::FileDialog::builder()
        .title("导出 Excel")
        .action(nwg::FileDialogAction::Save)
        .filters("Excel 工作簿(*.xlsx)")
        .build(&mut export_dialog)
        .expect("创建导出对话框失败");

    nwg::FileDialog::builder()
        .title("保存模板")
        .action(nwg::FileDialogAction::Save)
        .filters("Excel 工作簿(*.xlsx)")
        .build(&mut template_dialog)
        .expect("创建模板保存对话框失败");

    let imported_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let event_window = Rc::new(window);
    let event_window2 = event_window.clone();
    let imported_for_events = imported_path.clone();

    let handler = nwg::full_bind_event_handler(&event_window.handle, move |evt, _, handle| {
        use nwg::Event as E;

        match evt {
            E::OnWindowClose if &handle == &event_window2.handle => {
                nwg::stop_thread_dispatch();
            }
            E::OnButtonClick if &handle == &import_button => {
                if import_dialog.run(Some(&event_window2.handle)) {
                    match import_dialog.get_selected_item() {
                        Ok(path) => {
                            *imported_for_events.borrow_mut() = Some(PathBuf::from(path));
                            nwg::modal_info_message(&event_window2.handle, "导入", "Excel 已导入");
                        }
                        Err(e) => {
                            nwg::modal_error_message(
                                &event_window2.handle,
                                "错误",
                                &format!("读取所选文件失败：{e}"),
                            );
                        }
                    }
                }
            }
            E::OnButtonClick if &handle == &export_button => {
                let input = match imported_for_events.borrow().clone() {
                    Some(path) => path,
                    None => {
                        nwg::modal_error_message(
                            &event_window2.handle,
                            "提示",
                            "请先点击“导入”选择 Excel 文件",
                        );
                        return;
                    }
                };

                if export_dialog.run(Some(&event_window2.handle)) {
                    match export_dialog.get_selected_item() {
                        Ok(path) => {
                            let output = ensure_xlsx_extension(PathBuf::from(path));
                            match export_workbook(&input, &output) {
                                Ok(count) => {
                                    nwg::modal_info_message(
                                        &event_window2.handle,
                                        "完成",
                                        &format!("已生成 {count} 个二维码并导出 Excel"),
                                    );
                                }
                                Err(e) => {
                                    nwg::modal_error_message(
                                        &event_window2.handle,
                                        "导出失败",
                                        &format!("{e:#}"),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            nwg::modal_error_message(
                                &event_window2.handle,
                                "错误",
                                &format!("无法获取导出路径：{e}"),
                            );
                        }
                    }
                }
            }
            E::OnButtonClick if &handle == &template_button => {
                if template_dialog.run(Some(&event_window2.handle)) {
                    match template_dialog.get_selected_item() {
                        Ok(path) => {
                            let output = ensure_xlsx_extension(PathBuf::from(path));
                            match fs::write(&output, TEMPLATE_BYTES) {
                                Ok(_) => {
                                    nwg::modal_info_message(
                                        &event_window2.handle,
                                        "模板下载",
                                        "模板已保存",
                                    );
                                }
                                Err(e) => {
                                    nwg::modal_error_message(
                                        &event_window2.handle,
                                        "保存失败",
                                        &format!("{e}"),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            nwg::modal_error_message(
                                &event_window2.handle,
                                "错误",
                                &format!("无法获取保存路径：{e}"),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}

#[cfg(test)]
mod tests {
    use super::encode_label;

    #[test]
    fn known_vectors() {
        assert_eq!(encode_label("T3013225978").unwrap(), "A#V78HZ46DPdA");
        assert_eq!(encode_label("T3013224420").unwrap(), "A#V78HZ46CKY2");
        assert_eq!(encode_label("T3013149514").unwrap(), "A#V78HZ38HLX6");
    }
}
