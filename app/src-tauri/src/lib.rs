use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::ImageOutputFormat;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder,
};

use nsh_ai::{ChatClient, ChatClientConfig, ChatMessage};
use nsh_core::{AIConfig, AppConfig, AppPaths};
use nsh_ocr::BaiduOcrClient;
use nsh_poetry::{Poem, PoetryLibrary, SearchOptions};

const DEFAULT_MODEL_FETCH_TIMEOUT_SECS: u64 = 30;
const DEFAULT_POETRY_LIMIT: usize = 20;
const MAX_POETRY_LIMIT: usize = 100;
const AI_GENERAL_MAX_TOKENS: u32 = 128;
const AI_POETRY_MAX_TOKENS: u32 = 64;
const KNOWLEDGE_BASE_TIMEOUT_SECS: u64 = 10;
const AI_ANSWER_TIMEOUT_SECS: u64 = 10;
const SCREENSHOT_SELECTOR_LABEL: &str = "screenshot-selector";
const SCREENSHOT_HIDE_DELAY_MS: u64 = 500;
const FORCE_EXIT_GRACE_MS: u64 = 800;
const APP_BUILD_TAG: &str = "React v5.0 · 2026-04-27";

#[derive(Clone, Default)]
struct PoetryState {
    cache: Arc<Mutex<Option<CachedPoetryLibrary>>>,
}

struct CachedPoetryLibrary {
    db_path: PathBuf,
    library: PoetryLibrary,
}

#[derive(Clone, Default)]
struct ScreenshotSelectorState {
    last_bounds: Arc<Mutex<Option<ScreenshotBounds>>>,
}

#[derive(Debug, Serialize)]
struct AiPreset {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    default_model: &'static str,
}

#[derive(Debug, Serialize)]
struct AiModel {
    id: String,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigView {
    config_file: String,
    settings: nsh_core::Settings,
    ai: AiConfigView,
    ai_secondary: AiConfigView,
    baidu_ocr: BaiduOcrConfigView,
}

#[derive(Debug, Serialize)]
struct AiConfigView {
    provider: String,
    model: String,
    base_url: Option<String>,
    has_api_key: bool,
    api_key_redacted: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    timeout_secs: u64,
    use_primary_endpoint: bool,
}

#[derive(Debug, Serialize)]
struct BaiduOcrConfigView {
    app_id: Option<String>,
    has_api_key: bool,
    api_key_redacted: Option<String>,
    has_secret_key: bool,
    secret_key_redacted: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveConfigInput {
    settings: Option<nsh_core::Settings>,
    ai: Option<AiConfigInput>,
    ai_secondary: Option<AiConfigInput>,
    baidu_ocr: Option<BaiduOcrConfigInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiConfigInput {
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    timeout_secs: Option<u64>,
    use_primary_endpoint: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaiduOcrConfigInput {
    app_id: Option<String>,
    api_key: Option<String>,
    secret_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize)]
struct ScreenshotCapturedDto {
    data: String,
    bounds: ScreenshotBounds,
}

#[derive(Debug, Serialize)]
struct OcrTextDto {
    lines: Vec<String>,
    text: String,
}

#[derive(Debug, Serialize)]
struct PoemDto {
    id: i64,
    title: String,
    author: Option<String>,
    dynasty: Option<String>,
    paragraphs: Vec<String>,
    source: Option<String>,
    text: String,
}

#[derive(Debug, Serialize)]
struct PoemMatchDto {
    poem: PoemDto,
    score: f32,
    matched_chars: usize,
    matched_clause: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AiAnswerDto {
    answer: String,
    elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
struct AiChannelAnswerDto {
    channel: &'static str,
    label: &'static str,
    provider: String,
    model: String,
    answer: Option<String>,
    error: Option<String>,
    elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
struct AnswerTimingsDto {
    poetry_ms: Option<u128>,
    ai_ms: Option<u128>,
    total_ms: u128,
}

#[derive(Debug, Serialize)]
struct AnswerQuestionDto {
    question_type: &'static str,
    ai_question: String,
    poetry_query: Option<String>,
    poetry_results: Vec<PoemMatchDto>,
    ai_answer: Option<AiAnswerDto>,
    ai_error: Option<String>,
    ai_answers: Vec<AiChannelAnswerDto>,
    timings: AnswerTimingsDto,
}

#[tauri::command]
fn health() -> &'static str {
    APP_BUILD_TAG
}

#[tauri::command]
fn quit_app(
    app: AppHandle,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<(), String> {
    let _ = close_screenshot_selector_window(app.clone(), Some(&selector_state));
    request_app_exit(app);
    Ok(())
}

#[tauri::command]
fn get_ai_presets() -> Vec<AiPreset> {
    ai_presets()
}

#[tauri::command]
fn get_config() -> Result<ConfigView, String> {
    let path = config_path()?;
    let config = nsh_core::read_config(&path).map_err(command_error)?;
    Ok(config_view(config, path))
}

#[tauri::command]
fn save_config(input: SaveConfigInput) -> Result<ConfigView, String> {
    let path = config_path()?;
    let mut config = nsh_core::read_config(&path).map_err(command_error)?;

    if let Some(settings) = input.settings {
        config.settings = settings;
    }
    if let Some(ai) = input.ai {
        merge_ai_config(&mut config.ai, ai);
    }
    if let Some(ai_secondary) = input.ai_secondary {
        merge_ai_config(&mut config.ai_secondary, ai_secondary);
    }
    if let Some(baidu_ocr) = input.baidu_ocr {
        merge_baidu_ocr_config(&mut config, baidu_ocr);
    }

    nsh_core::write_config(&path, &config).map_err(command_error)?;
    Ok(config_view(config, path))
}

#[tauri::command]
async fn list_ai_models(
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Vec<AiModel>, String> {
    let saved = read_saved_config().unwrap_or_default();
    let provider = normalize_optional_string(provider).unwrap_or(saved.ai.provider);
    let base_url = normalize_optional_string(base_url).or(saved.ai.base_url);
    let api_key = normalize_optional_string(api_key).or(saved.ai.api_key);

    let Some(api_key) = api_key else {
        return Err("missing API key".to_string());
    };

    let client = ChatClient::new(ChatClientConfig {
        provider,
        base_url,
        api_key: Some(api_key),
        model: String::new(),
        max_tokens: None,
        temperature: None,
        timeout: Duration::from_secs(DEFAULT_MODEL_FETCH_TIMEOUT_SECS),
    })
    .map_err(command_error)?;

    let mut models: Vec<AiModel> = client
        .list_models()
        .await
        .map_err(command_error)?
        .into_iter()
        .map(|model| AiModel {
            id: model.id,
            name: model.name,
        })
        .collect();

    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

#[tauri::command]
async fn show_screenshot_selector(
    app: AppHandle,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SCREENSHOT_SELECTOR_LABEL) {
        window.show().map_err(command_error)?;
        window.set_focus().map_err(command_error)?;
        return Ok(());
    }

    let bounds = last_screenshot_bounds(&selector_state)?;
    let window = WebviewWindowBuilder::new(
        &app,
        SCREENSHOT_SELECTOR_LABEL,
        WebviewUrl::App("screenshot.html".into()),
    )
    .title("Screenshot Selector")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(true)
    .min_inner_size(50.0, 50.0)
    .inner_size(50.0, 50.0)
    .visible(false)
    .build()
    .map_err(command_error)?;

    window
        .set_position(Position::Physical(PhysicalPosition {
            x: bounds.x,
            y: bounds.y,
        }))
        .map_err(command_error)?;
    window
        .set_size(Size::Physical(PhysicalSize {
            width: bounds.width.max(50),
            height: bounds.height.max(50),
        }))
        .map_err(command_error)?;
    window.show().map_err(command_error)?;
    window.set_focus().map_err(command_error)?;

    Ok(())
}

#[tauri::command]
fn hide_screenshot_selector(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SCREENSHOT_SELECTOR_LABEL) {
        window.hide().map_err(command_error)?;
    }
    Ok(())
}

#[tauri::command]
fn close_screenshot_selector(
    app: AppHandle,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<(), String> {
    close_screenshot_selector_window(app, Some(&selector_state))
}

#[tauri::command]
fn get_screenshot_bounds(app: AppHandle) -> Result<ScreenshotBounds, String> {
    let window = app
        .get_webview_window(SCREENSHOT_SELECTOR_LABEL)
        .ok_or_else(|| "screenshot selector is not open".to_string())?;
    let position = window.outer_position().map_err(command_error)?;
    let size = window.outer_size().map_err(command_error)?;
    Ok(ScreenshotBounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

#[tauri::command]
fn set_screenshot_bounds(
    app: AppHandle,
    bounds: ScreenshotBounds,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(SCREENSHOT_SELECTOR_LABEL)
        .ok_or_else(|| "screenshot selector is not open".to_string())?;
    window
        .set_position(Position::Physical(PhysicalPosition {
            x: bounds.x,
            y: bounds.y,
        }))
        .map_err(command_error)?;
    window
        .set_size(Size::Physical(PhysicalSize {
            width: bounds.width.max(50),
            height: bounds.height.max(50),
        }))
        .map_err(command_error)?;
    remember_screenshot_bounds(&selector_state, bounds)?;
    Ok(())
}

#[tauri::command]
fn move_screenshot_selector(
    app: AppHandle,
    dx: i32,
    dy: i32,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<(), String> {
    let mut bounds = get_screenshot_bounds(app.clone())?;
    bounds.x += dx;
    bounds.y += dy;
    set_screenshot_bounds(app, bounds, selector_state)
}

#[tauri::command]
async fn capture_screenshot_from_selector(
    app: AppHandle,
    close_before_capture: Option<bool>,
    selector_state: tauri::State<'_, ScreenshotSelectorState>,
) -> Result<ScreenshotCapturedDto, String> {
    let bounds = get_screenshot_bounds(app.clone())?;
    remember_screenshot_bounds(&selector_state, bounds)?;
    if close_before_capture.unwrap_or(false) {
        close_screenshot_selector_window(app.clone(), Some(&selector_state))?;
    } else {
        hide_screenshot_selector(app.clone())?;
    }
    tokio::time::sleep(Duration::from_millis(SCREENSHOT_HIDE_DELAY_MS)).await;
    let data = capture_region_data_url(bounds)?;
    Ok(ScreenshotCapturedDto { data, bounds })
}

fn default_screenshot_bounds() -> ScreenshotBounds {
    ScreenshotBounds {
        x: 100,
        y: 100,
        width: 400,
        height: 300,
    }
}

fn last_screenshot_bounds(
    selector_state: &ScreenshotSelectorState,
) -> Result<ScreenshotBounds, String> {
    Ok(selector_state
        .last_bounds
        .lock()
        .map_err(|_| "screenshot selector state lock poisoned".to_string())?
        .unwrap_or_else(default_screenshot_bounds))
}

fn remember_screenshot_bounds(
    selector_state: &ScreenshotSelectorState,
    bounds: ScreenshotBounds,
) -> Result<(), String> {
    *selector_state
        .last_bounds
        .lock()
        .map_err(|_| "screenshot selector state lock poisoned".to_string())? =
        Some(ScreenshotBounds {
            width: bounds.width.max(50),
            height: bounds.height.max(50),
            ..bounds
        });
    Ok(())
}

fn close_screenshot_selector_window(
    app: AppHandle,
    selector_state: Option<&ScreenshotSelectorState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(SCREENSHOT_SELECTOR_LABEL) {
        if let Some(selector_state) = selector_state {
            if let Ok(position) = window.outer_position() {
                if let Ok(size) = window.outer_size() {
                    remember_screenshot_bounds(
                        selector_state,
                        ScreenshotBounds {
                            x: position.x,
                            y: position.y,
                            width: size.width,
                            height: size.height,
                        },
                    )?;
                }
            }
        }
        window.destroy().map_err(command_error)?;
    }
    Ok(())
}

fn request_app_exit(app: AppHandle) {
    app.exit(0);
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(FORCE_EXIT_GRACE_MS));
        std::process::exit(0);
    });
}

#[tauri::command]
fn capture_full_screen_base64() -> Result<String, String> {
    let screens = screenshots::Screen::all().map_err(command_error)?;
    let screen = screens
        .first()
        .ok_or_else(|| "no screen found".to_string())?;
    let image = screen.capture().map_err(command_error)?;
    rgba_image_to_data_url(image)
}

fn capture_region_data_url(bounds: ScreenshotBounds) -> Result<String, String> {
    let screens = screenshots::Screen::all().map_err(command_error)?;
    let screen = screens
        .iter()
        .find(|screen| {
            let info = screen.display_info;
            bounds.x >= info.x
                && bounds.y >= info.y
                && bounds.x < info.x + info.width as i32
                && bounds.y < info.y + info.height as i32
        })
        .or_else(|| screens.first())
        .ok_or_else(|| "no screen found".to_string())?;

    let info = screen.display_info;
    let relative_x = bounds.x - info.x;
    let relative_y = bounds.y - info.y;
    let image = screen
        .capture_area(
            relative_x,
            relative_y,
            bounds.width.max(1),
            bounds.height.max(1),
        )
        .map_err(command_error)?;
    rgba_image_to_data_url(image)
}

fn rgba_image_to_data_url(image: image::RgbaImage) -> Result<String, String> {
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, ImageOutputFormat::Png)
        .map_err(command_error)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(cursor.into_inner())
    ))
}

#[tauri::command]
async fn baidu_ocr_base64(
    image_base64: String,
    api_key: Option<String>,
    secret_key: Option<String>,
) -> Result<OcrTextDto, String> {
    let saved = read_saved_config().unwrap_or_default();
    let api_key = normalize_optional_string(api_key).or(saved.baidu_ocr.api_key);
    let secret_key = normalize_optional_string(secret_key).or(saved.baidu_ocr.secret_key);

    let Some(api_key) = api_key else {
        return Err("missing Baidu OCR API key".to_string());
    };
    let Some(secret_key) = secret_key else {
        return Err("missing Baidu OCR secret key".to_string());
    };

    let client = BaiduOcrClient::new(api_key, secret_key).map_err(command_error)?;
    let image_base64 = strip_data_url_prefix(&image_base64);
    let text = client
        .recognize_base64(image_base64)
        .await
        .map_err(command_error)?;

    Ok(OcrTextDto {
        lines: text.lines,
        text: text.text,
    })
}

#[tauri::command]
async fn search_poetry(
    query: String,
    limit: Option<usize>,
    poetry_state: tauri::State<'_, PoetryState>,
) -> Result<Vec<PoemDto>, String> {
    let limit = normalize_limit(limit);
    tokio::time::timeout(
        Duration::from_secs(KNOWLEDGE_BASE_TIMEOUT_SECS),
        with_cached_poetry_library(poetry_state.inner().clone(), move |library| {
            let poems = library.search(&query, SearchOptions { limit })?;
            Ok(poems.into_iter().map(poem_dto).collect::<Vec<_>>())
        }),
    )
    .await
    .map_err(|_| knowledge_base_timeout_error())?
}

#[tauri::command]
async fn match_poetry_from_text(
    text: String,
    limit: Option<usize>,
    poetry_state: tauri::State<'_, PoetryState>,
) -> Result<Vec<PoemMatchDto>, String> {
    let chars =
        nsh_poetry::clean_poem_chars(&text).unwrap_or_else(|| nsh_poetry::normalize_text(&text));
    tokio::time::timeout(
        Duration::from_secs(KNOWLEDGE_BASE_TIMEOUT_SECS),
        poetry_matches_from_chars(chars, normalize_limit(limit), poetry_state.inner().clone()),
    )
    .await
    .map_err(|_| knowledge_base_timeout_error())?
}

#[tauri::command]
async fn answer_question(
    question_text: String,
    poetry_state: tauri::State<'_, PoetryState>,
) -> Result<AnswerQuestionDto, String> {
    let total_start = Instant::now();
    let ai_question = question_text.trim().to_string();
    if ai_question.is_empty() {
        return Err("question is empty".to_string());
    }

    let poetry_query = nsh_poetry::clean_poem_chars(&ai_question);
    let question_type = if poetry_query.is_some() {
        "poetry"
    } else {
        "general"
    };

    let (poetry_results, poetry_ms) =
        timed_poetry_match(poetry_query.clone(), 10, poetry_state.inner().clone()).await?;

    if question_type == "poetry" && !poetry_results.is_empty() {
        return Ok(AnswerQuestionDto {
            question_type,
            ai_question,
            poetry_query,
            poetry_results,
            ai_answer: None,
            ai_error: None,
            ai_answers: Vec::new(),
            timings: AnswerTimingsDto {
                poetry_ms,
                ai_ms: None,
                total_ms: total_start.elapsed().as_millis(),
            },
        });
    }

    Ok(AnswerQuestionDto {
        question_type,
        ai_question,
        poetry_query,
        poetry_results,
        ai_answer: None,
        ai_error: None,
        ai_answers: Vec::new(),
        timings: AnswerTimingsDto {
            poetry_ms,
            ai_ms: None,
            total_ms: total_start.elapsed().as_millis(),
        },
    })
}

#[tauri::command]
async fn answer_ai_channel(
    question_text: String,
    channel: String,
) -> Result<AiChannelAnswerDto, String> {
    let ai_question = question_text.trim().to_string();
    if ai_question.is_empty() {
        return Err("question is empty".to_string());
    }

    let saved = read_saved_config().unwrap_or_default();
    let is_poetry_reference = nsh_poetry::clean_poem_chars(&ai_question).is_some();
    let (channel_id, label, config) = if channel.trim().eq_ignore_ascii_case("secondary") {
        (
            "secondary",
            "AI 2",
            resolve_ai_channel_config(&saved.ai, &saved.ai_secondary),
        )
    } else {
        ("primary", "AI 1", saved.ai)
    };

    Ok(run_ai_channel(channel_id, label, ai_question, config, is_poetry_reference).await)
}

fn resolve_ai_channel_config(primary: &AIConfig, channel: &AIConfig) -> AIConfig {
    if !channel.use_primary_endpoint {
        return channel.clone();
    }

    AIConfig {
        provider: primary.provider.clone(),
        base_url: primary.base_url.clone(),
        api_key: primary.api_key.clone(),
        model: channel.model.clone(),
        max_tokens: channel.max_tokens,
        temperature: channel.temperature,
        timeout_secs: channel.timeout_secs,
        use_primary_endpoint: channel.use_primary_endpoint,
    }
}

async fn run_ai_channel(
    channel: &'static str,
    label: &'static str,
    question_text: String,
    config: AIConfig,
    is_poetry_reference: bool,
) -> AiChannelAnswerDto {
    let provider = config.provider.clone();
    let model = config.model.clone();
    let started = Instant::now();
    match ask_ai_raw(question_text, config, is_poetry_reference).await {
        Ok(answer) => AiChannelAnswerDto {
            channel,
            label,
            provider,
            model,
            elapsed_ms: answer.elapsed_ms,
            answer: Some(answer.answer),
            error: None,
        },
        Err(error) => AiChannelAnswerDto {
            channel,
            label,
            provider,
            model,
            answer: None,
            error: Some(error),
            elapsed_ms: started.elapsed().as_millis(),
        },
    }
}

async fn timed_poetry_match(
    poetry_query: Option<String>,
    limit: usize,
    poetry_state: PoetryState,
) -> Result<(Vec<PoemMatchDto>, Option<u128>), String> {
    let Some(chars) = poetry_query else {
        return Ok((Vec::new(), None));
    };

    let started = Instant::now();
    let results = tokio::time::timeout(
        Duration::from_secs(KNOWLEDGE_BASE_TIMEOUT_SECS),
        poetry_matches_from_chars(chars, limit, poetry_state),
    )
    .await
    .map_err(|_| knowledge_base_timeout_error())??;
    Ok((results, Some(started.elapsed().as_millis())))
}

async fn poetry_matches_from_chars(
    chars: String,
    limit: usize,
    poetry_state: PoetryState,
) -> Result<Vec<PoemMatchDto>, String> {
    if chars.trim().is_empty() {
        return Ok(Vec::new());
    }

    with_cached_poetry_library(poetry_state, move |library| {
        let matches = library.find_poem_from_chars(&chars, limit)?;
        Ok(matches
            .into_iter()
            .map(|item| PoemMatchDto {
                poem: poem_dto(item.poem),
                score: item.score,
                matched_chars: item.matched_chars,
                matched_clause: item.matched_clause,
            })
            .collect::<Vec<_>>())
    })
    .await
}

async fn with_cached_poetry_library<T, F>(
    poetry_state: PoetryState,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&PoetryLibrary) -> std::result::Result<T, nsh_poetry::PoetryError> + Send + 'static,
{
    let db_path = resolve_poetry_db_path()?;
    let cache = poetry_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = cache
            .lock()
            .map_err(|_| "poetry library cache lock poisoned".to_string())?;
        let should_reload = guard
            .as_ref()
            .is_none_or(|cached| cached.db_path != db_path);
        if should_reload {
            let library = PoetryLibrary::open(&db_path).map_err(command_error)?;
            *guard = Some(CachedPoetryLibrary {
                db_path: db_path.clone(),
                library,
            });
        }
        let library = &guard
            .as_ref()
            .ok_or_else(|| "poetry library cache is empty".to_string())?
            .library;
        operation(library).map_err(command_error)
    })
    .await
    .map_err(command_error)?
}

async fn prewarm_poetry_library(poetry_state: PoetryState) -> Result<(), String> {
    with_cached_poetry_library(poetry_state, |library| {
        for chars in ["????????????", "????????????"] {
            let _ = library.find_poem_from_chars(chars, 1)?;
        }
        Ok(())
    })
    .await
}

async fn ask_ai_raw(
    question_text: String,
    config: AIConfig,
    is_poetry_reference: bool,
) -> Result<AiAnswerDto, String> {
    let started = Instant::now();
    let api_key = config
        .api_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "missing AI API key".to_string())?;
    if config.model.trim().is_empty() {
        return Err("missing AI model".to_string());
    }

    let token_cap = if is_poetry_reference {
        AI_POETRY_MAX_TOKENS
    } else {
        AI_GENERAL_MAX_TOKENS
    };
    let client = ChatClient::new(ChatClientConfig {
        provider: config.provider,
        base_url: config.base_url,
        api_key: Some(api_key),
        model: config.model,
        max_tokens: Some(config.max_tokens.unwrap_or(token_cap).min(token_cap)),
        temperature: Some(config.temperature.unwrap_or(0.2).min(0.7)),
        timeout: Duration::from_secs(config.timeout_secs.clamp(1, AI_ANSWER_TIMEOUT_SECS)),
    })
    .map_err(command_error)?;

    let system_prompt = if is_poetry_reference {
        "You answer poem character puzzles. Return only the most likely 5- or 7-character poem line. If unsure, give the best guess only."
    } else {
        "Answer directly and concisely. If it is a multiple-choice question, put the correct option first. Avoid long explanations."
    };

    let answer = tokio::time::timeout(
        Duration::from_secs(AI_ANSWER_TIMEOUT_SECS),
        client.chat_once(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(question_text),
        ]),
    )
    .await
    .map_err(|_| ai_answer_timeout_error())?
    .map_err(command_error)?;
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Err("AI returned empty answer".to_string());
    }

    Ok(AiAnswerDto {
        answer,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn ai_presets() -> Vec<AiPreset> {
    vec![
        AiPreset {
            id: "openai",
            name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            default_model: "gpt-5.4-mini",
        },
        AiPreset {
            id: "gemini",
            name: "Google Gemini",
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            default_model: "gemini-3-flash-preview",
        },
        AiPreset {
            id: "deepseek",
            name: "DeepSeek",
            base_url: "https://api.deepseek.com/v1",
            default_model: "deepseek-v4-flash",
        },
        AiPreset {
            id: "doubao",
            name: "Volcengine Ark / Doubao",
            base_url: "https://ark.cn-beijing.volces.com/api/v3",
            default_model: "doubao-seed-2-0-lite-260215",
        },
        AiPreset {
            id: "siliconflow",
            name: "SiliconFlow",
            base_url: "https://api.siliconflow.cn/v1",
            default_model: "Qwen/Qwen2.5-7B-Instruct",
        },
        AiPreset {
            id: "custom",
            name: "Custom",
            base_url: "",
            default_model: "",
        },
    ]
}

fn merge_ai_config(config: &mut AIConfig, input: AiConfigInput) {
    if let Some(provider) = normalize_optional_string(input.provider) {
        config.provider = provider;
    }
    if let Some(model) = normalize_optional_string(input.model) {
        config.model = model;
    }
    if input.base_url.is_some() {
        config.base_url = normalize_optional_string(input.base_url);
    }
    if input.api_key.is_some() {
        config.api_key = normalize_optional_string(input.api_key);
    }
    if input.max_tokens.is_some() {
        config.max_tokens = input.max_tokens;
    }
    if input.temperature.is_some() {
        config.temperature = input.temperature;
    }
    if let Some(timeout_secs) = input.timeout_secs.filter(|value| *value > 0) {
        config.timeout_secs = timeout_secs.min(AI_ANSWER_TIMEOUT_SECS);
    }
    if let Some(use_primary_endpoint) = input.use_primary_endpoint {
        config.use_primary_endpoint = use_primary_endpoint;
    }
}

fn merge_baidu_ocr_config(config: &mut AppConfig, input: BaiduOcrConfigInput) {
    if input.app_id.is_some() {
        config.baidu_ocr.app_id = normalize_optional_string(input.app_id);
    }
    if input.api_key.is_some() {
        config.baidu_ocr.api_key = normalize_optional_string(input.api_key);
    }
    if input.secret_key.is_some() {
        config.baidu_ocr.secret_key = normalize_optional_string(input.secret_key);
    }
}

fn config_view(config: AppConfig, config_file: PathBuf) -> ConfigView {
    ConfigView {
        config_file: config_file.display().to_string(),
        settings: config.settings,
        ai: ai_config_view(config.ai),
        ai_secondary: ai_config_view(config.ai_secondary),
        baidu_ocr: BaiduOcrConfigView {
            app_id: config.baidu_ocr.app_id,
            has_api_key: config
                .baidu_ocr
                .api_key
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            api_key_redacted: nsh_core::redact_optional_secret(config.baidu_ocr.api_key.as_deref()),
            has_secret_key: config
                .baidu_ocr
                .secret_key
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            secret_key_redacted: nsh_core::redact_optional_secret(
                config.baidu_ocr.secret_key.as_deref(),
            ),
        },
    }
}

fn ai_config_view(config: AIConfig) -> AiConfigView {
    AiConfigView {
        provider: config.provider,
        model: config.model,
        base_url: config.base_url,
        has_api_key: config
            .api_key
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        api_key_redacted: nsh_core::redact_optional_secret(config.api_key.as_deref()),
        max_tokens: config.max_tokens,
        temperature: config.temperature,
        timeout_secs: config.timeout_secs.min(AI_ANSWER_TIMEOUT_SECS),
        use_primary_endpoint: config.use_primary_endpoint,
    }
}

fn poem_dto(poem: Poem) -> PoemDto {
    let text = poem.text();
    PoemDto {
        id: poem.id,
        title: poem.title,
        author: poem.author,
        dynasty: poem.dynasty,
        paragraphs: poem.paragraphs,
        source: poem.source,
        text,
    }
}

fn config_path() -> Result<PathBuf, String> {
    let app_dir: Option<&Path> = None;
    let data_dir: Option<&Path> = None;
    Ok(AppPaths::resolve(app_dir, data_dir)
        .map_err(command_error)?
        .config_file)
}

fn read_saved_config() -> Result<AppConfig, String> {
    let path = config_path()?;
    nsh_core::read_config(path).map_err(command_error)
}

fn resolve_poetry_db_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("NSH_POETRY_DB") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    for candidate in poetry_db_candidates() {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("poetry database data/poetry.db was not found; set NSH_POETRY_DB if needed".to_string())
}

fn poetry_db_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        push_ancestor_data_paths(&mut candidates, &current_dir);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            push_ancestor_data_paths(&mut candidates, parent);
        }
    }
    if let Ok(paths) = AppPaths::resolve(None::<&Path>, None::<&Path>) {
        candidates.push(paths.data_dir.join("poetry.db"));
        candidates.push(paths.app_dir.join("data").join("poetry.db"));
    }
    candidates
}

fn push_ancestor_data_paths(candidates: &mut Vec<PathBuf>, start: &Path) {
    for ancestor in start.ancestors() {
        candidates.push(ancestor.join("data").join("poetry.db"));
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn normalize_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_POETRY_LIMIT)
        .clamp(1, MAX_POETRY_LIMIT)
}

fn knowledge_base_timeout_error() -> String {
    format!("local knowledge base timeout after {KNOWLEDGE_BASE_TIMEOUT_SECS}s")
}

fn ai_answer_timeout_error() -> String {
    format!("AI answer timeout after {AI_ANSWER_TIMEOUT_SECS}s")
}

fn strip_data_url_prefix(value: &str) -> &str {
    let value = value.trim();
    if value.starts_with("data:") {
        return value
            .split_once(',')
            .map_or(value, |(_, payload)| payload.trim());
    }
    value
}

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn run() {
    tauri::Builder::default()
        .manage(PoetryState::default())
        .manage(ScreenshotSelectorState::default())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                    return;
                };
                api.prevent_close();
                let app = window.app_handle().clone();
                let _ = close_screenshot_selector_window(app.clone(), None);
                request_app_exit(app);
            }
        })
        .setup(|app| {
            let poetry_state = app.state::<PoetryState>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let started = Instant::now();
                match prewarm_poetry_library(poetry_state).await {
                    Ok(()) => eprintln!(
                        "poetry library prewarmed in {} ms",
                        started.elapsed().as_millis()
                    ),
                    Err(error) => eprintln!("poetry library prewarm failed: {error}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            quit_app,
            get_ai_presets,
            get_config,
            save_config,
            list_ai_models,
            show_screenshot_selector,
            hide_screenshot_selector,
            close_screenshot_selector,
            get_screenshot_bounds,
            set_screenshot_bounds,
            move_screenshot_selector,
            capture_screenshot_from_selector,
            capture_full_screen_base64,
            baidu_ocr_base64,
            search_poetry,
            match_poetry_from_text,
            answer_question,
            answer_ai_channel
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
