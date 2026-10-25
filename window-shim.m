#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <dlfcn.h>

// Compiled by build.rs and linked into the binary. GPUI has no options for these, so the
// NSWindow is reached here directly.

// The app's window is told apart from AppKit's own borderless helper windows by its title bar.
static NSWindow *pomodoro_window(void) {
    for (NSWindow *window in NSApp.windows) {
        if (window.styleMask & NSWindowStyleMaskTitled) return window;
    }
    return nil;
}

long pomodoro_float(void) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    // macOS keeps a regular app's windows out of another app's full-screen Space, whatever
    // their level. An accessory app (no Dock icon, no menu bar) may float over it.
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    window.level = NSFloatingWindowLevel;
    // Follow the user to every Space, and sit over full-screen apps too.
    window.collectionBehavior |= NSWindowCollectionBehaviorCanJoinAllSpaces |
                                 NSWindowCollectionBehaviorFullScreenAuxiliary;
    window.hidesOnDeactivate = NO;
    [window orderFrontRegardless];
    return 1;
}

static void pin_layers(NSView *view) {
    if ([view.layer isKindOfClass:NSClassFromString(@"CAMetalLayer")]) {
        view.layer.contentsGravity = kCAGravityTopLeft;
    }
    for (NSView *subview in view.subviews) pin_layers(subview);
}

/** The card draws its own close button, so the traffic lights go, and its own drag. */
long pomodoro_hide_buttons(void) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    for (NSWindowButton b = NSWindowCloseButton; b <= NSWindowZoomButton; b++) {
        [window standardWindowButton:b].hidden = YES;
    }
    // The invisible title bar would still drag the window on its own, fighting the drag below; in
    // the 48 px pill it covers nearly everything, so the window shook. Only programmatic moves now.
    window.movable = NO;
    // AppKit's shadow, and the hairline it draws with it, follow the window's rectangle, not the
    // card; the card paints its own shadow.
    window.hasShadow = NO;
    // The (transparent) title bar still draws a hairline along the window's top edge; the
    // traffic lights live in it, so their container's container is the title bar to hide.
    window.titlebarSeparatorStyle = NSTitlebarSeparatorStyleNone;
    window.titleVisibility = NSWindowTitleHidden;
    NSView *titlebar = [window standardWindowButton:NSWindowCloseButton].superview.superview;
    if ([NSStringFromClass(titlebar.class) isEqualToString:@"NSTitlebarContainerView"]) titlebar.hidden = YES;
    // Until GPUI has drawn at a new size, the layer shows its last frame, stretched to fill the
    // window by default: a giant pill flashes as the card opens. Pinned to the top-left corner,
    // where the pill and the card both start, the last frame stays as it was.
    pin_layers(window.contentView);
    return 1;
}

// The window's spot is kept in its own defaults domain (a suite named after the .app's bundle id
// would be refused inside it).
static NSUserDefaults *pomodoro_defaults(void) {
    return [[NSUserDefaults alloc] initWithSuiteName:@"local.doodoro.window"];
}

// Saved and restored by the top-left corner: the window changes height when it is minimised to the
// pill, and AppKit's origin is the bottom-left one.
static void save_position(NSWindow *window) {
    NSPoint top_left = NSMakePoint(NSMinX(window.frame), NSMaxY(window.frame));
    [pomodoro_defaults() setObject:NSStringFromPoint(top_left) forKey:@"topLeft"];
}

// The screen `anchor` is mostly on (or the one with the menu bar).
static NSScreen *screen_under(NSRect anchor) {
    NSScreen *best = NSScreen.mainScreen;
    CGFloat best_area = 0;
    for (NSScreen *screen in NSScreen.screens) {
        NSRect overlap = NSIntersectionRect(screen.frame, anchor);
        if (overlap.size.width * overlap.size.height > best_area) {
            best = screen;
            best_area = overlap.size.width * overlap.size.height;
        }
    }
    return best;
}

// The frame moved, not resized, so that it fits inside the visible area of the screen `anchor` is
// on. The anchor is the pill when the card grows from it: by the card's own overlap, a card hanging
// off the side of a screen would be pulled onto the screen beside it.
static NSRect fit_on_screen(NSRect frame, NSRect anchor) {
    NSRect visible = screen_under(anchor).visibleFrame;
    frame.origin.x = MAX(NSMinX(visible), MIN(frame.origin.x, NSMaxX(visible) - frame.size.width));
    frame.origin.y = MAX(NSMinY(visible), MIN(frame.origin.y, NSMaxY(visible) - frame.size.height));
    return frame;
}

/**
 * Where the window was last dropped, for GPUI to open it there: the top-left corner relative to the
 * top-left of the screen it is mostly on (y down), and that screen's display id. Moved onto the
 * screen if it would hang off one; returns 0 on first run, or when the screen it was on has gone.
 */
int pomodoro_saved_position(double width, double height, double *x, double *y, unsigned int *display) {
    NSUserDefaults *defaults = pomodoro_defaults();
    NSString *saved = [defaults stringForKey:@"topLeft"];
    if (!saved) return 0;
    NSPoint top_left = NSPointFromString(saved);
    NSRect frame = NSMakeRect(top_left.x, top_left.y - height, width, height);
    BOOL on_screen = NO;
    for (NSScreen *screen in NSScreen.screens) {
        if (NSIntersectsRect(screen.visibleFrame, frame)) on_screen = YES;
    }
    // A spot whose screen has since gone is forgotten, rather than pulled onto another screen.
    if (!on_screen) return 0;
    // Kept on the screen of the corner it was dropped by, which may have been the pill's.
    NSRect corner = NSMakeRect(top_left.x, top_left.y - 1, 1, 1);
    frame = fit_on_screen(frame, NSIsEmptyRect(NSIntersectionRect(screen_under(corner).frame, corner)) ? frame : corner);
    NSScreen *best = screen_under(frame);
    *x = NSMinX(frame) - NSMinX(best.frame);
    *y = NSMaxY(best.frame) - NSMaxY(frame);
    *display = [best.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
    return 1;
}

// Where the pill sat before the card had to be pushed back onto the screen to open, so that
// minimising puts it back there. Forgotten once the card is dragged.
static NSPoint pill_top_left;
static BOOL pill_pushed;

// Where the window goes for content of width × height: its top-left corner kept, unless the card
// would then hang off the screen. Then it is nudged back onto it, and `pushed` is set, with the
// pill it grew from at `pill`.
static NSRect plan_resize(NSWindow *window, long width, long height, BOOL *pushed, NSPoint *pill) {
    NSPoint top_left = NSMakePoint(NSMinX(window.frame), NSMaxY(window.frame));
    NSRect frame = [window frameRectForContentRect:NSMakeRect(0, 0, width, height)];
    BOOL growing = frame.size.width * frame.size.height > window.frame.size.width * window.frame.size.height;
    if (!growing && pill_pushed) top_left = pill_top_left;
    *pushed = NO;
    frame.origin = NSMakePoint(top_left.x, top_left.y - frame.size.height);
    if (growing) {
        NSRect fitted = fit_on_screen(frame, window.frame);
        if (!NSEqualPoints(fitted.origin, frame.origin)) {
            *pill = top_left;
            *pushed = YES;
            frame = fitted;
        }
    }
    return frame;
}

/** Resizes the content to width × height, where plan_resize puts it. */
long pomodoro_resize(long width, long height) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    BOOL pushed;
    NSPoint pill;
    NSRect frame = plan_resize(window, width, height, &pushed, &pill);
    pill_pushed = pushed;
    if (pushed) pill_top_left = pill;
    [window setFrame:frame display:YES];
    // The frame drawn for the new size goes out with the window's new frame in this transaction,
    // but GPUI's next frame is presented as soon as it is drawn. Committed now, before that frame,
    // the window doesn't show the new picture in its old place (the pill cut off beside itself).
    [CATransaction flush];
    return 1;
}

/**
 * Where the pill will be from the window's top-left corner (x right, y down) once it is resized to
 * width × height, so that the card can be drawn growing from there in the first frame at that size.
 */
long pomodoro_pill_after_resize(long width, long height, double *x, double *y) {
    NSWindow *window = pomodoro_window();
    *x = *y = 0;
    if (!window) return 0;
    BOOL pushed;
    NSPoint pill;
    NSRect frame = plan_resize(window, width, height, &pushed, &pill);
    if (pushed) {
        *x = pill.x - NSMinX(frame);
        *y = NSMaxY(frame) - pill.y;
    }
    return 1;
}

// Where the pill is, or will go back to, from the window's top-left corner (x right, y down):
// off the corner only while the card is pushed. The morph between them starts or ends there.
long pomodoro_pill_x(void) {
    NSWindow *window = pomodoro_window();
    return window && pill_pushed ? lround(pill_top_left.x - NSMinX(window.frame)) : 0;
}

long pomodoro_pill_y(void) {
    NSWindow *window = pomodoro_window();
    return window && pill_pushed ? lround(NSMaxY(window.frame) - pill_top_left.y) : 0;
}

// A drag polls the pointer rather than waiting on GPUI's mouse events, which stop arriving
// once the window starts moving under them.
static NSPoint drag_mouse, drag_origin;
static BOOL drag_moved;

long pomodoro_drag_begin(void) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    drag_mouse = NSEvent.mouseLocation;
    drag_origin = window.frame.origin;
    drag_moved = NO;
    return 1;
}

/** Moves the window with the pointer; returns 0 once the button is up, and keeps the spot. */
long pomodoro_drag_step(void) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    if ((NSEvent.pressedMouseButtons & 1) == 0) {
        if (drag_moved) save_position(window);
        return 0;
    }
    NSPoint m = NSEvent.mouseLocation;
    if (fabs(m.x - drag_mouse.x) + fabs(m.y - drag_mouse.y) > 3) {
        drag_moved = YES;
        pill_pushed = NO;
    }
    [window setFrameOrigin:NSMakePoint(drag_origin.x + m.x - drag_mouse.x, drag_origin.y + m.y - drag_mouse.y)];
    return 1;
}

/** Whether the last press dragged the window, as opposed to a click. */
long pomodoro_drag_moved(void) {
    return drag_moved;
}

/** Shows the window once its chrome is set up, without taking focus from the frontmost app. */
long pomodoro_show(void) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    [window orderFrontRegardless];
    return 1;
}

/**
 * Writes the window, as drawn, to a PNG (DOODORO_SCREENSHOT). A process may capture its own
 * windows without the screen-recording permission. CGWindowListCreateImage is gone from the
 * headers of recent SDKs but still in CoreGraphics, so it is looked up at run time.
 */
long pomodoro_screenshot(const char *path) {
    NSWindow *window = pomodoro_window();
    if (!window) return 0;
    typedef CGImageRef (*Capture)(CGRect, uint32_t, uint32_t, uint32_t);
    Capture capture = (Capture)dlsym(RTLD_DEFAULT, "CGWindowListCreateImage");
    if (!capture) return 0;
    // kCGWindowListOptionIncludingWindow, kCGWindowImageBoundsIgnoreFraming
    CGImageRef image = capture(CGRectNull, 1 << 3, (uint32_t)window.windowNumber, 1 << 0);
    if (!image) return 0;
    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithCGImage:image];
    CGImageRelease(image);
    NSData *png = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    return [png writeToFile:[NSString stringWithUTF8String:path] atomically:YES];
}
