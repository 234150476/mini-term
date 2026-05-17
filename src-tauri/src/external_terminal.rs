use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[derive(Clone, Default)]
pub struct ExternalTerminalState {
    titles_by_project: Arc<Mutex<HashMap<String, String>>>,
    dock_state: Arc<Mutex<Option<DockState>>>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Debug, Clone)]
struct WindowGeometry {
    actual: WindowRect,
    visible: WindowRect,
}

#[derive(Debug, Clone, Copy)]
enum DockSide {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct DockState {
    side: DockSide,
    wt_hwnd: usize,
    last_wt_rect: WindowRect,
    last_app_rect: WindowRect,
    ignore_app_moves_until_ms: u128,
}

impl ExternalTerminalState {
    pub fn new() -> Self {
        Self::default()
    }
}

const WT_WINDOW_NAME: &str = "mini-term-companion";
const MIN_SIDEBAR_WIDTH: i32 = 220;
const MIN_WT_WIDTH: i32 = 480;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn rect_width(rect: &WindowRect) -> i32 {
    rect.right - rect.left
}

fn rect_height(rect: &WindowRect) -> i32 {
    rect.bottom - rect.top
}

#[cfg(target_os = "windows")]
fn main_window_info(app: &tauri::AppHandle) -> Result<(usize, WindowGeometry), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "failed to get main webview window".to_string())?;
    let hwnd = window
        .hwnd()
        .map_err(|e| format!("hwnd failed: {e}"))?;
    let hwnd_raw = hwnd.0 as usize;
    let geometry = win::get_window_geometry(hwnd_raw)?;
    Ok((hwnd_raw, geometry))
}

#[cfg(target_os = "windows")]
mod win {
    use crate::external_terminal::{WindowGeometry, WindowRect, WT_WINDOW_NAME};
    use windows::core::BSTR;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
        IUIAutomationElementArray, TreeScope_Children, TreeScope_Descendants, UIA_PROPERTY_ID,
        UIA_ClassNamePropertyId, UIA_ControlTypePropertyId, UIA_TabItemControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    #[derive(Clone)]
    struct TerminalWindowInfo {
        name: String,
        titles: Vec<String>,
        rect: WindowRect,
        hwnd: usize,
    }

    pub struct ComGuard;

    impl ComGuard {
        pub fn init() -> Result<Self, String> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if hr.is_err() {
                    return Err(format!("CoInitializeEx failed: {hr:?}"));
                }
            }
            Ok(Self)
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn create_automation() -> Result<IUIAutomation, String> {
        unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("CoCreateInstance(CUIAutomation) failed: {e}"))
        }
    }

    fn property_condition(
        automation: &IUIAutomation,
        property_id: UIA_PROPERTY_ID,
        value: windows::core::VARIANT,
    ) -> Result<IUIAutomationCondition, String> {
        unsafe {
            automation
                .CreatePropertyCondition(property_id, &value)
                .map_err(|e| format!("CreatePropertyCondition failed: {e}"))
        }
    }

    fn element_name(el: &IUIAutomationElement) -> Option<String> {
        unsafe { el.CurrentName().ok().map(|b: BSTR| b.to_string()) }
    }

    fn read_tab_titles_from_window(
        automation: &IUIAutomation,
        window: &IUIAutomationElement,
    ) -> Result<Vec<String>, String> {
        unsafe {
            let tab_cond = property_condition(
                automation,
                UIA_ControlTypePropertyId,
                windows::core::VARIANT::from(UIA_TabItemControlTypeId.0),
            )?;
            let tabs: IUIAutomationElementArray = window
                .FindAll(TreeScope_Descendants, &tab_cond)
                .map_err(|e| format!("FindAll(TabItem) failed: {e}"))?;
            let len = tabs
                .Length()
                .map_err(|e| format!("Tab Length failed: {e}"))?;
            let mut out = Vec::new();
            for i in 0..len {
                let tab = tabs
                    .GetElement(i)
                    .map_err(|e| format!("GetElement(tab) failed: {e}"))?;
                if let Some(name) = element_name(&tab) {
                    out.push(name);
                }
            }
            Ok(out)
        }
    }

    fn list_terminal_windows(
        automation: &IUIAutomation,
    ) -> Result<Vec<TerminalWindowInfo>, String> {
        unsafe {
            let root = automation
                .GetRootElement()
                .map_err(|e| format!("GetRootElement failed: {e}"))?;
            let class_cond = property_condition(
                automation,
                UIA_ClassNamePropertyId,
                windows::core::VARIANT::from("CASCADIA_HOSTING_WINDOW_CLASS"),
            )?;
            let windows = root
                .FindAll(TreeScope_Children, &class_cond)
                .map_err(|e| format!("FindAll(Children) failed: {e}"))?;

            let len = windows
                .Length()
                .map_err(|e| format!("Length failed: {e}"))?;
            let mut out = Vec::new();
            for i in 0..len {
                let win = windows
                    .GetElement(i)
                    .map_err(|e| format!("GetElement failed: {e}"))?;
                let name = element_name(&win).unwrap_or_default();
                let titles = read_tab_titles_from_window(automation, &win)?;
                let hwnd = win
                    .CurrentNativeWindowHandle()
                    .map_err(|e| format!("CurrentNativeWindowHandle failed: {e}"))?;
                let geometry = get_window_geometry(hwnd.0 as usize)?;
                out.push(TerminalWindowInfo {
                    name,
                    titles,
                    rect: geometry.visible,
                    hwnd: hwnd.0 as usize,
                });
            }
            Ok(out)
        }
    }

    pub fn list_companion_tab_titles(preferred_titles: &[String]) -> Result<Vec<String>, String> {
        let _guard = ComGuard::init()?;
        let automation = create_automation()?;
        let windows = list_terminal_windows(&automation)?;
        if windows.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(info) = windows
            .iter()
            .find(|info| info.name.contains(WT_WINDOW_NAME))
        {
            return Ok(info.titles.clone());
        }

        if !preferred_titles.is_empty() {
            let mut best_score = 0usize;
            let mut best_titles: Option<Vec<String>> = None;
            for info in &windows {
                let score = info
                    .titles
                    .iter()
                    .filter(|title| preferred_titles.iter().any(|preferred| preferred == *title))
                    .count();
                if score > best_score {
                    best_score = score;
                    best_titles = Some(info.titles.clone());
                }
            }
            if best_score > 0 {
                return Ok(best_titles.unwrap_or_default());
            }
        }

        if windows.len() == 1 {
            return Ok(windows[0].titles.clone());
        }

        Ok(Vec::new())
    }

    pub fn find_companion_window_rect(preferred_titles: &[String]) -> Result<Option<WindowRect>, String> {
        let _guard = ComGuard::init()?;
        let automation = create_automation()?;
        let windows = list_terminal_windows(&automation)?;
        if windows.is_empty() {
            return Ok(None);
        }

        if let Some(info) = windows
            .iter()
            .find(|info| info.name.contains(WT_WINDOW_NAME))
        {
            return Ok(Some(info.rect.clone()));
        }

        if !preferred_titles.is_empty() {
            let mut best_score = 0usize;
            let mut best_rect: Option<WindowRect> = None;
            for info in &windows {
                let score = info
                    .titles
                    .iter()
                    .filter(|title| preferred_titles.iter().any(|preferred| preferred == *title))
                    .count();
                if score > best_score {
                    best_score = score;
                    best_rect = Some(info.rect.clone());
                }
            }
            if best_score > 0 {
                return Ok(best_rect);
            }
        }

        if windows.len() == 1 {
            return Ok(Some(windows[0].rect.clone()));
        }

        Ok(None)
    }

    pub fn find_companion_window_info(
        preferred_titles: &[String],
    ) -> Result<Option<(WindowRect, usize)>, String> {
        let _guard = ComGuard::init()?;
        let automation = create_automation()?;
        let windows = list_terminal_windows(&automation)?;
        if windows.is_empty() {
            return Ok(None);
        }

        if let Some(info) = windows
            .iter()
            .find(|info| info.name.contains(WT_WINDOW_NAME))
        {
            return Ok(Some((info.rect.clone(), info.hwnd)));
        }

        if !preferred_titles.is_empty() {
            let mut best_score = 0usize;
            let mut best: Option<(WindowRect, usize)> = None;
            for info in &windows {
                let score = info
                    .titles
                    .iter()
                    .filter(|title| preferred_titles.iter().any(|preferred| preferred == *title))
                    .count();
                if score > best_score {
                    best_score = score;
                    best = Some((info.rect.clone(), info.hwnd));
                }
            }
            if best_score > 0 {
                return Ok(best);
            }
        }

        if windows.len() == 1 {
            let info = &windows[0];
            return Ok(Some((info.rect.clone(), info.hwnd)));
        }

        Ok(None)
    }

    pub fn move_and_resize_window(hwnd_raw: usize, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|e| format!("SetWindowPos(move_and_resize) failed: {e}"))?;
            Ok(())
        }
    }

    fn get_actual_window_rect(hwnd_raw: usize) -> Result<WindowRect, String> {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).map_err(|e| format!("GetWindowRect failed: {e}"))?;
            Ok(WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        }
    }

    fn get_visible_window_rect(hwnd_raw: usize) -> Result<WindowRect, String> {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            let mut frame_rect = RECT::default();
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame_rect as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .map_err(|e| format!("DwmGetWindowAttribute failed: {e}"))?;
            Ok(WindowRect {
                left: frame_rect.left,
                top: frame_rect.top,
                right: frame_rect.right,
                bottom: frame_rect.bottom,
            })
        }
    }

    pub fn get_window_geometry(hwnd_raw: usize) -> Result<WindowGeometry, String> {
        Ok(WindowGeometry {
            actual: get_actual_window_rect(hwnd_raw)?,
            visible: get_visible_window_rect(hwnd_raw)?,
        })
    }

    pub fn is_zoomed(hwnd_raw: usize) -> bool {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
            IsZoomed(hwnd).as_bool()
        }
    }
}

#[cfg(target_os = "windows")]
fn focus_tab_by_index(tab_index: usize) -> Result<bool, String> {
    let mut cmd = Command::new("wt");
    cmd.arg("-w")
        .arg(WT_WINDOW_NAME)
        .arg("focus-tab")
        .arg("-t")
        .arg(tab_index.to_string());
    let status = cmd.status().map_err(|e| format!("failed to focus wt tab: {e}"))?;
    Ok(status.success())
}

#[cfg(target_os = "windows")]
fn open_new_tab(project_path: &str, title: &str) -> Result<(), String> {
    let mut cmd = Command::new("wt");
    cmd.arg("-w")
        .arg(WT_WINDOW_NAME)
        .arg("nt")
        .arg("-d")
        .arg(project_path)
        .arg("--title")
        .arg(title)
        .arg("--suppressApplicationTitle");
    let status = cmd.status().map_err(|e| format!("failed to launch wt: {e}"))?;
    if !status.success() {
        return Err(format!("wt exited with status {status}"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn choose_title(base_name: &str, project_id: &str, titles_by_project: &HashMap<String, String>, open_titles: &[String]) -> String {
    if let Some(existing) = titles_by_project.get(project_id) {
        return existing.clone();
    }

    let used: HashSet<String> = open_titles.iter().cloned().collect();
    if !used.contains(base_name) {
        return base_name.to_string();
    }

    let mut idx = 1usize;
    loop {
        let candidate = format!("{base_name}({idx})");
        if !used.contains(&candidate) {
            return candidate;
        }
        idx += 1;
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn activate_project_terminal(
    state: tauri::State<'_, ExternalTerminalState>,
    project_id: String,
    project_path: String,
    project_name: String,
) -> Result<(), String> {
    let preferred_titles = {
        let guard = state
            .titles_by_project
            .lock()
            .map_err(|_| "failed to lock external terminal state".to_string())?;
        guard.values().cloned().collect::<Vec<_>>()
    };

    let open_titles = win::list_companion_tab_titles(&preferred_titles)?;
    let mut titles_by_project = state
        .titles_by_project
        .lock()
        .map_err(|_| "failed to lock external terminal state".to_string())?;

    if open_titles.is_empty() {
        titles_by_project.clear();
    }

    if let Some(existing_title) = titles_by_project.get(&project_id).cloned() {
        if let Some(tab_index) = open_titles.iter().position(|title| title == &existing_title) {
            if focus_tab_by_index(tab_index)? {
                return Ok(());
            }
        } else {
            titles_by_project.remove(&project_id);
        }
    }

    let title = choose_title(&project_name, &project_id, &titles_by_project, &open_titles);
    open_new_tab(&project_path, &title)?;
    titles_by_project.insert(project_id, title);
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn get_companion_window_rect(
    state: tauri::State<'_, ExternalTerminalState>,
) -> Result<Option<WindowRect>, String> {
    let preferred_titles = {
        let guard = state
            .titles_by_project
            .lock()
            .map_err(|_| "failed to lock external terminal state".to_string())?;
        guard.values().cloned().collect::<Vec<_>>()
    };
    win::find_companion_window_rect(&preferred_titles)
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn clear_companion_dock(
    state: tauri::State<'_, ExternalTerminalState>,
) -> Result<(), String> {
    let mut guard = state
        .dock_state
        .lock()
        .map_err(|_| "failed to lock dock state".to_string())?;
    *guard = None;
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn try_snap_companion_dock(
    app: tauri::AppHandle,
    state: tauri::State<'_, ExternalTerminalState>,
    threshold: i32,
    _gap: i32,
) -> Result<bool, String> {
    let preferred_titles = {
        let guard = state
            .titles_by_project
            .lock()
            .map_err(|_| "failed to lock external terminal state".to_string())?;
        guard.values().cloned().collect::<Vec<_>>()
    };

    let Some((wt_rect, wt_hwnd)) = win::find_companion_window_info(&preferred_titles)? else {
        return Ok(false);
    };
    let (app_hwnd, app_geometry) = main_window_info(&app)?;
    let app_actual = &app_geometry.actual;
    let app_visible = &app_geometry.visible;
    let frame_left = app_visible.left - app_actual.left;
    let frame_top = app_visible.top - app_actual.top;
    let frame_right = app_actual.right - app_visible.right;
    let frame_bottom = app_actual.bottom - app_visible.bottom;
    let visible_width = rect_width(app_visible);

    let left_dist = (app_visible.right - wt_rect.left).abs();
    let right_dist = (app_visible.left - wt_rect.right).abs();
    let vertical_overlap = !(app_visible.top > wt_rect.bottom || app_visible.top + 48 < wt_rect.top);
    if !vertical_overlap {
        return Ok(false);
    }

    let side = if left_dist <= threshold {
        Some(DockSide::Left)
    } else if right_dist <= threshold {
        Some(DockSide::Right)
    } else {
        None
    };

    let Some(side) = side else {
        return Ok(false);
    };

    let is_zoomed = win::is_zoomed(wt_hwnd);
    let visible_width = visible_width.max(MIN_SIDEBAR_WIDTH);
    if is_zoomed {
        return Ok(false);
    }

    let snapped_visible_left = match side {
        DockSide::Left => wt_rect.left - visible_width,
        DockSide::Right => wt_rect.right,
    };
    let actual_x = snapped_visible_left - frame_left;
    let actual_y = wt_rect.top - frame_top;
    let actual_width = visible_width + frame_left + frame_right;
    let actual_height = rect_height(&wt_rect) + frame_top + frame_bottom;
    win::move_and_resize_window(
        app_hwnd,
        actual_x,
        actual_y,
        actual_width.max(1),
        actual_height.max(1),
    )?;

    let mut dock_guard = state
        .dock_state
        .lock()
        .map_err(|_| "failed to lock dock state".to_string())?;
    *dock_guard = Some(DockState {
        side,
        wt_hwnd,
        last_wt_rect: wt_rect.clone(),
        last_app_rect: WindowRect {
            left: actual_x,
            top: actual_y,
            right: actual_x + actual_width,
            bottom: actual_y + actual_height,
        },
        ignore_app_moves_until_ms: now_ms() + 500,
    });
    Ok(true)
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn handle_docked_app_moved(
    app: tauri::AppHandle,
    state: tauri::State<'_, ExternalTerminalState>,
) -> Result<bool, String> {
    let mut dock_guard = state
        .dock_state
        .lock()
        .map_err(|_| "failed to lock dock state".to_string())?;
    let Some(dock) = dock_guard.as_mut() else {
        return Ok(false);
    };
    if now_ms() < dock.ignore_app_moves_until_ms {
        return Ok(false);
    }

    let wt_rect = win::get_window_geometry(dock.wt_hwnd)?.visible;
    let (_app_hwnd, app_geometry) = main_window_info(&app)?;
    let app_actual = app_geometry.actual;
    let app_visible = app_geometry.visible;
    let visible_height = rect_height(&app_visible);
    let wt_width = rect_width(&wt_rect).max(MIN_WT_WIDTH);
    let wt_height = visible_height.max(1);

    let new_wt_x = match dock.side {
        DockSide::Left => app_visible.right,
        DockSide::Right => app_visible.left - wt_width,
    };
    let new_wt_y = app_visible.top;
    win::move_and_resize_window(dock.wt_hwnd, new_wt_x, new_wt_y, wt_width, wt_height)?;
    dock.last_wt_rect = WindowRect {
        left: new_wt_x,
        top: new_wt_y,
        right: new_wt_x + wt_width,
        bottom: new_wt_y + wt_height,
    };
    dock.last_app_rect = app_actual;
    Ok(true)
}

#[cfg(target_os = "windows")]
pub fn start_dock_monitor(app: tauri::AppHandle, state: ExternalTerminalState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(120));

        let maybe_dock = {
            let guard = state.dock_state.lock();
            match guard {
                Ok(guard) => guard.clone(),
                Err(_) => None,
            }
        };
        let Some(mut dock) = maybe_dock else {
            continue;
        };

        let is_zoomed = win::is_zoomed(dock.wt_hwnd);
        if is_zoomed {
            if let Ok(mut guard) = state.dock_state.lock() {
                *guard = None;
            }
            continue;
        }
        let Ok(wt_geometry) = win::get_window_geometry(dock.wt_hwnd) else {
            continue;
        };
        let wt_rect = wt_geometry.visible;
        if wt_rect.left == dock.last_wt_rect.left
            && wt_rect.top == dock.last_wt_rect.top
            && wt_rect.right == dock.last_wt_rect.right
            && wt_rect.bottom == dock.last_wt_rect.bottom
        {
            continue;
        }

        let Ok((app_hwnd, app_geometry)) = main_window_info(&app) else {
            continue;
        };
        let app_actual = app_geometry.actual;
        let app_visible = app_geometry.visible;
        let frame_left = app_visible.left - app_actual.left;
        let frame_right = app_actual.right - app_visible.right;
        let frame_top = app_visible.top - app_actual.top;
        let frame_bottom = app_actual.bottom - app_visible.bottom;
        let last_visible_width =
            rect_width(&dock.last_app_rect).max(MIN_SIDEBAR_WIDTH);
        let delta_left = wt_rect.left - dock.last_wt_rect.left;
        let delta_right = wt_rect.right - dock.last_wt_rect.right;
        let moved_whole = delta_left == delta_right;

        let mut visible_width = last_visible_width;
        let visible_left = match dock.side {
            DockSide::Left => {
                if delta_left != 0 && delta_right == 0 {
                    visible_width = (wt_rect.left - (dock.last_app_rect.left + frame_left))
                        .max(MIN_SIDEBAR_WIDTH);
                    wt_rect.left - visible_width
                } else {
                    wt_rect.left - visible_width
                }
            }
            DockSide::Right => {
                if delta_right != 0 && delta_left == 0 {
                    let right_fixed = dock.last_app_rect.right - frame_right;
                    visible_width = (right_fixed - wt_rect.right).max(MIN_SIDEBAR_WIDTH);
                    right_fixed - visible_width
                } else {
                    wt_rect.right
                }
            }
        };
        let visible_top = wt_rect.top;

        dock.last_wt_rect = wt_rect.clone();

        let actual_x = visible_left - frame_left;
        let actual_y = visible_top - frame_top;
        let actual_width = visible_width + frame_left + frame_right;
        let actual_height = rect_height(&dock.last_wt_rect) + frame_top + frame_bottom;
        let _ = win::move_and_resize_window(
            app_hwnd,
            actual_x,
            actual_y,
            actual_width.max(1),
            actual_height.max(1),
        );

        if moved_whole {
            dock.last_wt_rect = wt_rect;
        }
        dock.last_app_rect = WindowRect {
            left: actual_x,
            top: actual_y,
            right: actual_x + actual_width,
            bottom: actual_y + actual_height,
        };
        dock.ignore_app_moves_until_ms = now_ms() + 500;
        if let Ok(mut guard) = state.dock_state.lock() {
            *guard = Some(dock);
        }
    });
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn activate_project_terminal(
    _state: tauri::State<'_, ExternalTerminalState>,
    _project_id: String,
    _project_path: String,
    _project_name: String,
) -> Result<(), String> {
    Err("external Windows Terminal integration is only supported on Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn get_companion_window_rect(
    _state: tauri::State<'_, ExternalTerminalState>,
) -> Result<Option<WindowRect>, String> {
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn clear_companion_dock(
    _state: tauri::State<'_, ExternalTerminalState>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn try_snap_companion_dock(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, ExternalTerminalState>,
    _threshold: i32,
    _gap: i32,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn handle_docked_app_moved(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, ExternalTerminalState>,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(target_os = "windows"))]
pub fn start_dock_monitor(_app: tauri::AppHandle, _state: ExternalTerminalState) {}
