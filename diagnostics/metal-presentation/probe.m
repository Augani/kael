#import <AppKit/AppKit.h>
#import <Metal/Metal.h>
#import <MetalKit/MetalKit.h>
#import <QuartzCore/CAMetalLayer.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdatomic.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// An independent control: no Kael, Rust, or custom shader code. Default mode
// uses Apple's MTKView; --layer uses Kael's makeBackingLayer arrangement.
// The process succeeds only with a real positive MTLDrawable.presentedTime.
@interface ProbeLayerView : NSView
@property(nonatomic, strong) CAMetalLayer *renderLayer;
@end
@implementation ProbeLayerView
- (CALayer *)makeBackingLayer { return self.renderLayer; }
@end

@interface Probe : NSObject <NSApplicationDelegate, MTKViewDelegate> {
    atomic_uint _completed;
    atomic_uint _callbacks;
    atomic_uint _presented;
    atomic_uint _dropped;
    atomic_uint _failures;
}
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) NSView *view;
@property(nonatomic, strong) id<MTLDevice> device;
@property(nonatomic, strong) id<MTLCommandQueue> queue;
@property(nonatomic, strong) NSTimer *timer;
@property(nonatomic) BOOL layerMode;
@property(nonatomic) BOOL transactionMode;
@property(nonatomic) BOOL coverDisplay;
@property(nonatomic) BOOL registerAfterPresent;
@property(nonatomic) NSUInteger submitted;
@property(nonatomic) NSUInteger ticks;
@property(nonatomic) CFTimeInterval frameDeadline;
@property(nonatomic) int result;
@end

static void PrintScreens(void) {
    CFDictionaryRef session = CGSessionCopyCurrentDictionary();
    if (session) {
        CFTypeRef onConsole = CFDictionaryGetValue(session, kCGSessionOnConsoleKey);
        CFTypeRef loginDone = CFDictionaryGetValue(session, kCGSessionLoginDoneKey);
        // Diagnostic-only key: absence is unknown, never evidence of unlock.
        CFTypeRef locked = CFDictionaryGetValue(session, CFSTR("CGSSessionScreenIsLocked"));
        fprintf(stdout, "CONTROL_SESSION on_console=%d login_done=%d locked_known=%d locked=%d\n",
            onConsole == kCFBooleanTrue, loginDone == kCFBooleanTrue, locked != NULL,
            locked == kCFBooleanTrue);
        CFRelease(session);
    } else {
        fprintf(stdout, "CONTROL_SESSION unavailable=1\n");
    }
    for (NSScreen *screen in NSScreen.screens) {
        CGDirectDisplayID display = [screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
        CGDisplayModeRef mode = CGDisplayCopyDisplayMode(display);
        fprintf(stdout, "CONTROL_SCREEN id=%u frame=%s visible=%s scale=%.3f online=%d active=%d asleep=%d builtin=%d mirror=%d pixels=%zux%zu refresh=%.3f\n",
            display, NSStringFromRect(screen.frame).UTF8String,
            NSStringFromRect(screen.visibleFrame).UTF8String, screen.backingScaleFactor,
            CGDisplayIsOnline(display), CGDisplayIsActive(display), CGDisplayIsAsleep(display),
            CGDisplayIsBuiltin(display), CGDisplayIsInMirrorSet(display),
            mode ? CGDisplayModeGetPixelWidth(mode) : 0,
            mode ? CGDisplayModeGetPixelHeight(mode) : 0,
            mode ? CGDisplayModeGetRefreshRate(mode) : 0.0);
        if (mode) CGDisplayModeRelease(mode);
    }
}

@implementation Probe
- (void)printState:(const char *)stage {
    CAMetalLayer *layer = (CAMetalLayer *)self.view.layer;
    BOOL sameLayer = !self.layerMode || ((ProbeLayerView *)self.view).renderLayer == layer;
    CGDirectDisplayID display = [self.window.screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
    fprintf(stdout, "CONTROL_STATE stage=%s app_active=%d window_visible=%d key=%d main=%d occlusion=%lu display=%u window_frame=%s content_bounds=%s view_frame=%s view_hidden=%d attached=%d same_layer=%d layer_frame=%s layer_bounds=%s layer_hidden=%d opacity=%.3f drawable=%.0fx%.0f scale=%.3f vsync=%d transaction=%d\n",
        stage, NSApp.active, self.window.visible, self.window.keyWindow, self.window.mainWindow,
        (unsigned long)self.window.occlusionState, display,
        NSStringFromRect(self.window.frame).UTF8String,
        NSStringFromRect(self.window.contentView.bounds).UTF8String,
        NSStringFromRect(self.view.frame).UTF8String, self.view.hidden,
        self.view.window == self.window, sameLayer,
        NSStringFromRect(layer.frame).UTF8String, NSStringFromRect(layer.bounds).UTF8String,
        layer.hidden, layer.opacity, layer.drawableSize.width, layer.drawableSize.height,
        layer.contentsScale, layer.displaySyncEnabled, layer.presentsWithTransaction);
}

- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    (void)notification;
    PrintScreens();
    self.device = MTLCreateSystemDefaultDevice();
    self.queue = [self.device newCommandQueue];
    if (!self.device || !self.queue || !NSScreen.mainScreen) {
        fprintf(stderr, "CONTROL_FAIL cannot_create_device_queue_or_screen\n");
        self.result = 2;
        [NSApp terminate:nil];
        return;
    }
    // The macOS 27 SDK deprecates isHeadless/isLowPower/isRemovable as not
    // applicable on Apple Silicon. Do not infer real display state from them.
    fprintf(stdout, "CONTROL_DEVICE name=%s registry_id=%llu unified=%d\n",
        self.device.name.UTF8String, self.device.registryID, self.device.hasUnifiedMemory);
    NSRect frame = self.coverDisplay ? NSScreen.mainScreen.frame : NSMakeRect(0, 0, 720, 460);
    NSWindowStyleMask style = self.coverDisplay ? NSWindowStyleMaskBorderless :
        (NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable);
    self.window = [[NSWindow alloc] initWithContentRect:frame styleMask:style
        backing:NSBackingStoreBuffered defer:NO screen:NSScreen.mainScreen];
    self.window.releasedWhenClosed = NO;
    self.window.title = @"Independent Metal presentation control";
    self.window.level = NSFloatingWindowLevel;
    if (!self.coverDisplay) [self.window center];
    if (self.layerMode) {
        ProbeLayerView *view = [[ProbeLayerView alloc] initWithFrame:self.window.contentView.bounds];
        view.renderLayer = [CAMetalLayer layer];
        view.renderLayer.device = self.device;
        view.renderLayer.pixelFormat = MTLPixelFormatBGRA8Unorm;
        view.renderLayer.opaque = NO;
        view.renderLayer.maximumDrawableCount = 3;
        view.renderLayer.autoresizingMask = kCALayerWidthSizable | kCALayerHeightSizable;
        view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        view.wantsLayer = YES;
        view.layerContentsRedrawPolicy = NSViewLayerContentsRedrawDuringViewResize;
        self.view = view;
    } else {
        MTKView *view = [[MTKView alloc] initWithFrame:self.window.contentView.bounds device:self.device];
        view.colorPixelFormat = MTLPixelFormatBGRA8Unorm;
        view.paused = YES;
        view.enableSetNeedsDisplay = NO;
        view.delegate = self;
        self.view = view;
    }
    [self.window.contentView addSubview:self.view];
    CAMetalLayer *layer = (CAMetalLayer *)self.view.layer;
    layer.presentsWithTransaction = self.transactionMode;
    layer.contentsScale = self.window.backingScaleFactor;
    layer.drawableSize = CGSizeMake(self.view.bounds.size.width * layer.contentsScale,
        self.view.bounds.size.height * layer.contentsScale);
    [self.window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
    [self printState:"opened"];
    __weak Probe *weakSelf = self;
    // Give AppKit time to attach/activate the layer before the first command.
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 300 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        Probe *probe = weakSelf;
        if (!probe) return;
        [probe printState:"before_frames"];
        probe.frameDeadline = CACurrentMediaTime() + 6.0;
        probe.timer = [NSTimer scheduledTimerWithTimeInterval:1.0/30.0 repeats:YES block:^(NSTimer *timer) {
            (void)timer;
            [weakSelf tick];
        }];
    });
}

- (void)tick {
    self.ticks += 1;
    if (self.submitted >= 60 || self.ticks > 90 || CACurrentMediaTime() >= self.frameDeadline) {
        [self.timer invalidate];
        self.timer = nil;
        __weak Probe *weakSelf = self;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
            [weakSelf finish];
        });
        return;
    }
    @autoreleasepool {
        if (self.layerMode) {
            CAMetalLayer *layer = (CAMetalLayer *)self.view.layer;
            id<CAMetalDrawable> drawable = [layer nextDrawable];
            if (!drawable) { atomic_fetch_add(&_failures, 1); return; }
            MTLRenderPassDescriptor *descriptor = [MTLRenderPassDescriptor renderPassDescriptor];
            descriptor.colorAttachments[0].texture = drawable.texture;
            descriptor.colorAttachments[0].loadAction = MTLLoadActionClear;
            descriptor.colorAttachments[0].storeAction = MTLStoreActionStore;
            [self draw:drawable descriptor:descriptor];
        } else {
            [(MTKView *)self.view draw];
        }
    }
}

- (void)drawInMTKView:(MTKView *)view {
    MTLRenderPassDescriptor *descriptor = view.currentRenderPassDescriptor;
    id<CAMetalDrawable> drawable = view.currentDrawable;
    if (!descriptor || !drawable) { atomic_fetch_add(&_failures, 1); return; }
    [self draw:drawable descriptor:descriptor];
}
- (void)mtkView:(MTKView *)view drawableSizeWillChange:(CGSize)size {
    (void)view; (void)size;
}

- (void)draw:(id<CAMetalDrawable>)drawable descriptor:(MTLRenderPassDescriptor *)descriptor {
    const NSUInteger index = self.submitted++;
    descriptor.colorAttachments[0].clearColor = MTLClearColorMake(
        (index % 3) == 0 ? 0.8 : 0.1, (index % 3) == 1 ? 0.8 : 0.1,
        (index % 3) == 2 ? 0.8 : 0.1, 1.0);
    id<MTLCommandBuffer> buffer = [self.queue commandBuffer];
    id<MTLRenderCommandEncoder> encoder = [buffer renderCommandEncoderWithDescriptor:descriptor];
    if (!buffer || !encoder) { atomic_fetch_add(&_failures, 1); return; }
    [encoder endEncoding];
    __weak Probe *weakSelf = self;
    [buffer addCompletedHandler:^(id<MTLCommandBuffer> completed) {
        Probe *probe = weakSelf;
        if (!probe) return;
        atomic_fetch_add(&probe->_completed, 1);
        if (completed.status != MTLCommandBufferStatusCompleted) atomic_fetch_add(&probe->_failures, 1);
        fprintf(stdout, "CONTROL_COMPLETE frame=%lu status=%lu gpu_start=%.9f gpu_end=%.9f\n",
            (unsigned long)index, (unsigned long)completed.status, completed.GPUStartTime, completed.GPUEndTime);
    }];
    MTLDrawablePresentedHandler handler = ^(id<MTLDrawable> presented) {
        Probe *probe = weakSelf;
        if (!probe) return;
        const double time = presented.presentedTime;
        atomic_fetch_add(&probe->_callbacks, 1);
        if (isfinite(time) && time > 0) atomic_fetch_add(&probe->_presented, 1);
        else atomic_fetch_add(&probe->_dropped, 1);
        fprintf(stdout, "CONTROL_PRESENTED frame=%lu drawable_id=%lu presented_time=%.9f\n",
            (unsigned long)index, (unsigned long)presented.drawableID, time);
    };
    if (!self.registerAfterPresent || self.transactionMode) [drawable addPresentedHandler:handler];
    if (self.transactionMode) {
        [buffer commit];
        [buffer waitUntilScheduled];
        [drawable present];
    } else {
        [buffer presentDrawable:drawable];
        if (self.registerAfterPresent) [drawable addPresentedHandler:handler];
        [buffer commit];
    }
}

- (void)finish {
    [self printState:"finished"];
    const unsigned completed = atomic_load(&_completed);
    const unsigned callbacks = atomic_load(&_callbacks);
    const unsigned presented = atomic_load(&_presented);
    const unsigned dropped = atomic_load(&_dropped);
    const unsigned failures = atomic_load(&_failures);
    const BOOL geometry = self.view.window == self.window && !self.view.hidden && self.window.visible &&
        self.view.bounds.size.width >= 700 && self.view.bounds.size.height >= 400;
    const BOOL ok = self.submitted == 60 && completed == 60 && presented > 0 && failures == 0 && geometry;
    self.result = ok ? 0 : 1;
    fprintf(stdout, "CONTROL_RESULT mode=%s transaction=%d handler_after_present=%d submitted=%lu completed=%u callbacks=%u presented=%u dropped=%u failures=%u geometry=%d result=%s\n",
        self.layerMode ? "layer" : "mtk", self.transactionMode, self.registerAfterPresent,
        (unsigned long)self.submitted, completed, callbacks, presented, dropped, failures, geometry,
        ok ? "PASS" : "FAIL");
    [NSApp terminate:nil];
}
- (void)applicationWillTerminate:(NSNotification *)notification {
    (void)notification;
    // AppKit's terminate: does not normally return from NSApplication.run.
    // A failed control must therefore remain a real nonzero CLI result.
    fflush(NULL);
    exit(self.result);
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        setbuf(stdout, NULL);
        if (argc == 2 && !strcmp(argv[1], "--diagnose")) {
            PrintScreens();
            return 0;
        }
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        Probe *probe = [Probe new];
        probe.result = 2;
        for (int i = 1; i < argc; ++i) {
            if (!strcmp(argv[i], "--layer")) probe.layerMode = YES;
            else if (!strcmp(argv[i], "--transaction")) probe.transactionMode = YES;
            else if (!strcmp(argv[i], "--cover-display")) probe.coverDisplay = YES;
            else if (!strcmp(argv[i], "--handler-after-present")) probe.registerAfterPresent = YES;
            else { fprintf(stderr, "unknown argument: %s\n", argv[i]); return 2; }
        }
        fprintf(stdout, "CONTROL_START mode=%s transaction=%d cover_display=%d handler_after_present=%d frames=60\n",
            probe.layerMode ? "layer" : "mtk", probe.transactionMode, probe.coverDisplay, probe.registerAfterPresent);
        NSApp.delegate = probe;
        [NSApp run];
        return probe.result;
    }
}
