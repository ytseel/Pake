//! Cross-platform webview navigation that does not depend on page JS.
//! Blank error shells have no JS context, so `eval("history.back()")` and
//! `location.reload()` are dead exactly when the user is stuck.

use tauri::WebviewWindow;


/// Install Brave's request filter on a Windows WebView2 instance.
///
/// The callback is registered for all resource contexts so EasyList rules can
/// match scripts, images, media, XHR/fetch, frames and document requests.
#[cfg(windows)]
pub fn install_adblock(
    webview: &tauri::webview::PlatformWebview,
    state: std::sync::Arc<crate::adblock::AdblockState>,
) {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
            ICoreWebView2_2,
        },
        WebResourceRequestedEventHandler,
    };
    use windows_core::{w, Interface};

    let controller = webview.controller();
    let core = match unsafe { controller.CoreWebView2() } {
        Ok(core) => core,
        Err(error) => {
            eprintln!("[Pake Adblock] Cannot access WebView2: {error}");
            return;
        }
    };
    let core2 = match core.cast::<ICoreWebView2_2>() {
        Ok(core2) => core2,
        Err(error) => {
            eprintln!("[Pake Adblock] WebView2 request interception is unavailable: {error}");
            return;
        }
    };

    let environment = match unsafe { core2.Environment() } {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("[Pake Adblock] Cannot access WebView2 environment: {error}");
            return;
        }
    };

    let filter_added = unsafe {
        core2.AddWebResourceRequestedFilter(
            w!("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )
    };
    if let Err(error) = filter_added {
        eprintln!("[Pake Adblock] Cannot register request filter: {error}");
        return;
    }

    let callback_state = std::sync::Arc::clone(&state);
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };

        let request = unsafe { args.Request()? };

        // webview2-com 0.38 exposes these COM getters with caller-provided
        // output pointers rather than returning their values directly.
        let mut request_url = windows_core::PWSTR::null();
        let mut method = windows_core::PWSTR::null();
        let mut source_url = windows_core::PWSTR::null();
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT(0);
        unsafe {
            request.Uri(&mut request_url)?;
            request.Method(&mut method)?;
            sender.Source(&mut source_url)?;
            args.ResourceContext(&mut context)?;
        }
        let request_url = unsafe { request_url.to_string()? };
        let method = unsafe { method.to_string()? };
        let source_url = unsafe { source_url.to_string()? };

        let request_type = if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT {
            "document"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET {
            "stylesheet"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE {
            "image"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT {
            "script"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
            || context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH
            || context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE
        {
            "xmlhttprequest"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA
            || context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK
        {
            "media"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT {
            "font"
        } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING {
            "ping"
        } else {
            "other"
        };

        if callback_state.should_block(&request_url, &source_url, request_type, &method) {
            // An empty 403 response prevents the blocked resource from reaching
            // the network. It is deliberately not a blanket domain block:
            // YouTube media endpoints remain subject to the actual filter rules.
            let response = unsafe {
                environment.CreateWebResourceResponse(
                    None,
                    403,
                    w!("Blocked by Pake"),
                    w!("Content-Type: text/plain\r\n"),
                )?
            };
            unsafe { args.SetResponse(&response)? };
        }

        Ok(())
    }));

    let mut token = 0i64;
    if let Err(error) = unsafe { core2.add_WebResourceRequested(&handler, &mut token) } {
        eprintln!("[Pake Adblock] Cannot attach WebView2 request handler: {error}");
        return;
    }

    eprintln!("[Pake Adblock] Brave network filtering enabled.");
}

/// Reload the current document via the native webview API.
pub fn reload_window(window: &WebviewWindow) {
    if let Err(error) = window.reload() {
        eprintln!("[Pake] Failed to reload webview: {error}");
    }
}

/// Step the webview history without requiring a live page JS context.
pub fn history_step(window: &WebviewWindow, back: bool) {
    let stepped = window.with_webview(move |webview| {
        history_step_platform(&webview, back);
    });

    if stepped.is_err() {
        // Last resort when with_webview is unavailable (rare). Still fails on
        // blank error shells, but preserves SPA history when the page is live.
        let script = if back {
            "window.history.back()"
        } else {
            "window.history.forward()"
        };
        if let Err(error) = window.eval(script) {
            eprintln!("[Pake] Failed to step webview history: {error}");
        }
    }
}

/// Apply the password-save prompt setting, including explicit opt-out.
///
/// This deliberately configures the app's own WebView2 profile; it does not
/// access or share credentials with a Microsoft Edge profile.
#[cfg(windows)]
pub fn set_password_autosave(webview: &tauri::webview::PlatformWebview, enabled: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
    use windows_core::Interface;

    let controller = webview.controller();
    let result = unsafe {
        controller
            .CoreWebView2()
            .and_then(|core| core.Settings())
            .and_then(|settings| settings.cast::<ICoreWebView2Settings4>())
            .and_then(|settings| settings.SetIsPasswordAutosaveEnabled(enabled))
    };

    if let Err(error) = result {
        eprintln!("[Pake] Failed to configure WebView2 password autosave: {error}");
    }
}

/// Let a two-finger horizontal trackpad swipe step history, as in Safari.
///
/// WKWebView leaves `allowsBackForwardNavigationGestures` off, and Tauri does
/// not expose Wry's builder switch, so set it on the native view directly.
#[cfg(target_os = "macos")]
pub fn enable_back_forward_gestures(webview: &tauri::webview::PlatformWebview) {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};

    let ptr = webview.inner() as *mut AnyObject;
    if ptr.is_null() {
        eprintln!("[Pake] Failed to access WKWebView for history swipe gestures");
        return;
    }
    unsafe {
        let _: () = msg_send![ptr, setAllowsBackForwardNavigationGestures: Bool::YES];
    }
}

#[cfg(target_os = "macos")]
fn history_step_platform(webview: &tauri::webview::PlatformWebview, back: bool) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let ptr = webview.inner() as *mut AnyObject;
    if ptr.is_null() {
        return;
    }
    unsafe {
        if back {
            let _: *mut AnyObject = msg_send![ptr, goBack];
        } else {
            let _: *mut AnyObject = msg_send![ptr, goForward];
        }
    }
}

#[cfg(target_os = "linux")]
fn history_step_platform(webview: &tauri::webview::PlatformWebview, back: bool) {
    use webkit2gtk::WebViewExt;

    let gtk_webview = webview.inner();
    if back {
        gtk_webview.go_back();
    } else {
        gtk_webview.go_forward();
    }
}

#[cfg(windows)]
fn history_step_platform(webview: &tauri::webview::PlatformWebview, back: bool) {
    let controller = webview.controller();
    unsafe {
        // ICoreWebView2Controller::CoreWebView2 returns the underlying browser.
        let Ok(core) = controller.CoreWebView2() else {
            return;
        };
        if back {
            let _ = core.GoBack();
        } else {
            let _ = core.GoForward();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn history_step_platform(_webview: &tauri::webview::PlatformWebview, _back: bool) {}
