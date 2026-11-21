// Native Sparkle boundary. Never start without configured public feed/key.
#import <AppKit/AppKit.h>
#import <Sparkle/Sparkle.h>

extern bool jaywalk_updater_restart_is_safe(void);
static BOOL restartAllowed = NO;
static BOOL checksAllowed = NO;
static BOOL downloadsAllowed = NO;
static BOOL installsAllowed = NO;
static NSString *lastStatus = @"No update requests made.";

@interface JaywalkUpdateDriver : SPUStandardUserDriver <SPUUpdaterDelegate, NSURLSessionDownloadDelegate>
@property(nonatomic, strong) SPUUpdater *updater;
@property(nonatomic, strong) NSURLSession *session;
@property(nonatomic, strong) NSURLSessionDownloadTask *download;
@property(nonatomic, strong) SUAppcastItem *downloadItem;
@property(nonatomic, strong) NSWindow *progressWindow;
@property(nonatomic, strong) NSProgressIndicator *progress;
@property(nonatomic) BOOL showDownloadResult;
@property(nonatomic, copy) void (^pendingRelaunch)(void);
@end

@implementation JaywalkUpdateDriver
- (BOOL)updaterShouldPromptForPermissionToCheckForUpdates:(SPUUpdater *)updater { return NO; }
- (BOOL)updater:(SPUUpdater *)updater mayPerformUpdateCheck:(SPUUpdateCheck)check error:(NSError **)error {
    return check == SPUUpdateCheckUpdates || checksAllowed;
}
- (BOOL)updater:(SPUUpdater *)updater shouldDownloadReleaseNotesForUpdate:(SUAppcastItem *)item { return NO; }
- (BOOL)updater:(SPUUpdater *)updater shouldProceedWithUpdate:(SUAppcastItem *)item updateCheck:(SPUUpdateCheck)check error:(NSError **)error {
    if (!jaywalk_updater_restart_is_safe() && installsAllowed) {
        if (error) *error = [NSError errorWithDomain:@"dev.jaywalk.Updater" code:1 userInfo:@{NSLocalizedDescriptionKey:@"Finish waiting CLI sessions and restore local review checkouts before installing an update."}];
        return NO;
    }
    return YES;
}
- (BOOL)updater:(SPUUpdater *)updater shouldPostponeRelaunchForUpdate:(SUAppcastItem *)item untilInvokingBlock:(void (^)(void))handler {
    // Resume only after a fresh explicit check and confirmation, never a timer.
    if (!jaywalk_updater_restart_is_safe()) {
        self.pendingRelaunch = handler;
        lastStatus = @"Update restart deferred: finish CLI sessions and restore local review checkouts, then check again.";
        return YES;
    }
    return NO;
}
- (void)showReadyToInstallAndRelaunch:(void (^)(SPUUserUpdateChoice))reply {
    NSAlert *alert=[NSAlert new];alert.messageText=@"Ready to update Jaywalk";
    alert.informativeText=@"Restart to install the verified update, or cancel this installation for now. Download-only archives remain separate and are never installed on quit.";
    [alert addButtonWithTitle:@"Later"];[alert addButtonWithTitle:@"Restart to update"];
    if ([alert runModal] == NSAlertSecondButtonReturn && jaywalk_updater_restart_is_safe()) reply(SPUUserUpdateChoiceInstall);
    else reply(SPUUserUpdateChoiceSkip); // At this stage Skip cancels installation; it does not skip the version.
}
- (void)cancelDownload:(id)sender {
    [self.download cancel]; self.download = nil;
    [self.progressWindow close]; self.progressWindow = nil;
    lastStatus = @"Update download canceled.";
}
+ (BOOL)allowedURL:(NSURL *)url {
    if (![url.scheme isEqualToString:@"https"]) return NO;
    if ([url.host isEqualToString:@"github.com"]) return [url.path hasPrefix:@"/sorahn/jaywalk/releases/download/"];
    return [url.host isEqualToString:@"release-assets.githubusercontent.com"];
}
- (NSURL *)cacheDirectory {
    return [[NSFileManager.defaultManager URLsForDirectory:NSCachesDirectory inDomains:NSUserDomainMask].firstObject URLByAppendingPathComponent:@"dev.jaywalk.Jaywalk/updates" isDirectory:YES];
}
- (void)downloadOnly:(SUAppcastItem *)item userInitiated:(BOOL)manual {
    // This path only stores bytes; it never extracts, launches an installer or
    // schedules installation on termination. Sparkle verifies again at install.
    if (self.download != nil || item.contentLength > 1024ULL*1024*1024 || ![JaywalkUpdateDriver allowedURL:item.fileURL]) return;
    NSDictionary *cached=[NSDictionary dictionaryWithContentsOfURL:[[self cacheDirectory] URLByAppendingPathComponent:@"download.plist"] error:nil];
    NSDictionary *attributes=[NSFileManager.defaultManager attributesOfItemAtPath:[[[self cacheDirectory] URLByAppendingPathComponent:@"downloaded-update.zip"] path] error:nil];
    if ([cached[@"version"] isEqual:item.versionString] && [cached[@"url"] isEqual:item.fileURL.absoluteString] && [attributes[NSFileSize] unsignedLongLongValue] == item.contentLength) {
        lastStatus=@"This update archive is already downloaded. Installation remains a separate verified action.";
        return;
    }
    self.downloadItem = item; self.showDownloadResult = manual;
    if (!self.session) self.session = [NSURLSession sessionWithConfiguration:NSURLSessionConfiguration.ephemeralSessionConfiguration delegate:self delegateQueue:NSOperationQueue.mainQueue];
    self.download = [self.session downloadTaskWithURL:item.fileURL];
    lastStatus = @"Downloading update without installing…";
    if (manual) {
        self.progressWindow = [[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,420,100) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        self.progressWindow.title = @"Downloading Jaywalk update";
        self.progressWindow.releasedWhenClosed = NO;
        self.progress = [[NSProgressIndicator alloc] initWithFrame:NSMakeRect(20,55,380,16)];
        self.progress.indeterminate = NO; self.progress.maxValue = 1;
        [self.progressWindow.contentView addSubview:self.progress];
        NSButton *cancel = [NSButton buttonWithTitle:@"Cancel" target:self action:@selector(cancelDownload:)];
        cancel.frame = NSMakeRect(300,10,100,30);
        [self.progressWindow.contentView addSubview:cancel];
        [self.progressWindow center]; [self.progressWindow makeKeyAndOrderFront:nil];
    }
    [self.download resume];
}
- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task willPerformHTTPRedirection:(NSHTTPURLResponse *)response newRequest:(NSURLRequest *)request completionHandler:(void (^)(NSURLRequest *))completion {
    completion([JaywalkUpdateDriver allowedURL:request.URL] ? request : nil);
}
- (void)URLSession:(NSURLSession *)session downloadTask:(NSURLSessionDownloadTask *)task didWriteData:(int64_t)bytes totalBytesWritten:(int64_t)total totalBytesExpectedToWrite:(int64_t)expected {
    if (total > 1024LL*1024*1024) { [self cancelDownload:nil]; return; }
    self.progress.doubleValue = expected > 0 ? (double)total / expected : 0;
}
- (void)URLSession:(NSURLSession *)session downloadTask:(NSURLSessionDownloadTask *)task didFinishDownloadingToURL:(NSURL *)location {
    if (task != self.download) return;
    if (![task.response isKindOfClass:NSHTTPURLResponse.class] || ((NSHTTPURLResponse *)task.response).statusCode != 200) {lastStatus=@"Update download failed: unexpected server response.";return;}
    NSFileManager *files = NSFileManager.defaultManager;
    NSURL *directory = [self cacheDirectory];
    NSError *error = nil;
    [files createDirectoryAtURL:directory withIntermediateDirectories:YES attributes:nil error:&error];
    NSURL *stage = [directory URLByAppendingPathComponent:@"downloaded-update.zip"];
    if (!error) { [files removeItemAtURL:stage error:nil]; [files moveItemAtURL:location toURL:stage error:&error]; }
    if (!error) {
        NSDictionary *metadata=@{@"version":self.downloadItem.versionString,@"url":self.downloadItem.fileURL.absoluteString};
        [metadata writeToURL:[directory URLByAppendingPathComponent:@"download.plist"] error:&error];
    }
    lastStatus = error ? @"Could not save downloaded update." : @"Update downloaded; nothing will install on quit. Choose Check for updates to install when ready. Sparkle will re-fetch and verify the release before installation.";
    [self.progressWindow close]; self.progressWindow=nil;
    if (self.showDownloadResult) {
        NSAlert *alert=[NSAlert new]; alert.messageText=@"Update download";alert.informativeText=lastStatus;[alert addButtonWithTitle:@"OK"];[alert runModal];
    }
}
- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task didCompleteWithError:(NSError *)error {
    if (task != self.download) return;
    if (error) lastStatus = error.code == NSURLErrorCancelled ? @"Update download canceled." : @"Update download failed; check your connection and try again.";
    self.download=nil;[self.progressWindow close];self.progressWindow=nil;
    if (error && self.showDownloadResult && error.code != NSURLErrorCancelled) {
        NSAlert *alert=[NSAlert new];alert.messageText=@"Update download failed";alert.informativeText=lastStatus;[alert addButtonWithTitle:@"OK"];[alert runModal];
    }
}
- (void)showUpdateFoundWithAppcastItem:(SUAppcastItem *)item state:(SPUUserUpdateState *)state reply:(void (^)(SPUUserUpdateChoice))reply {
    if (!state.userInitiated && !checksAllowed) {reply(SPUUserUpdateChoiceDismiss);return;}
    if (!state.userInitiated && downloadsAllowed && !installsAllowed) {
        [self downloadOnly:item userInitiated:NO];reply(SPUUserUpdateChoiceDismiss);return;
    }
    if (state.stage == SPUUserUpdateStageNotDownloaded) {
        NSAlert *alert = [NSAlert new];alert.messageText=@"A Jaywalk update is available";
        alert.informativeText=@"Download only keeps an archive without installing, even on quit. Install update continues through Sparkle's verified installation and restart flow.";
        [alert addButtonWithTitle:@"Later"];[alert addButtonWithTitle:@"Download only"];[alert addButtonWithTitle:@"Install update…"];
        NSModalResponse choice=[alert runModal];
        if (choice == NSAlertSecondButtonReturn) {[self downloadOnly:item userInitiated:YES];reply(SPUUserUpdateChoiceDismiss);return;}
        if (choice != NSAlertThirdButtonReturn) {reply(SPUUserUpdateChoiceDismiss);return;}
    }
    if (!jaywalk_updater_restart_is_safe()) {
        lastStatus=@"Restore active local review checkouts and finish waiting CLI sessions before installing.";
        NSAlert *alert=[NSAlert new];alert.messageText=@"Update deferred";alert.informativeText=lastStatus;[alert addButtonWithTitle:@"OK"];[alert runModal];reply(SPUUserUpdateChoiceDismiss);return;
    }
    reply(SPUUserUpdateChoiceInstall);
}
@end

static JaywalkUpdateDriver *driver;
static BOOL initialize(void) {
    if (driver) return YES;
    NSBundle *bundle=NSBundle.mainBundle;
    NSString *feed=[bundle objectForInfoDictionaryKey:@"SUFeedURL"];
    NSString *key=[bundle objectForInfoDictionaryKey:@"SUPublicEDKey"];
    if (feed.length==0 || key.length==0) {lastStatus=@"Updates are not configured for this build.";return NO;}
    driver=[[JaywalkUpdateDriver alloc] initWithHostBundle:bundle delegate:nil];
    driver.updater=[[SPUUpdater alloc] initWithHostBundle:bundle applicationBundle:bundle userDriver:driver delegate:driver];
    driver.updater.sendsSystemProfile=NO;
    driver.updater.automaticallyChecksForUpdates=NO;
    driver.updater.automaticallyDownloadsUpdates=NO;
    NSError *error=nil;
    if (![driver.updater startUpdater:&error]) {lastStatus=@"The updater could not start; reinstall a supported release.";driver=nil;return NO;}
    lastStatus=[@"Update feed: " stringByAppendingString:feed];
    return YES;
}
void jaywalk_updater_configure(bool consent, bool checks, bool downloads, bool install, double interval, bool restart) {
    restartAllowed=restart;checksAllowed=consent && checks;downloadsAllowed=checksAllowed && downloads;installsAllowed=downloadsAllowed && install && restart;
    if (!checksAllowed && !driver) return;
    if (!initialize()) return;
    if (!downloadsAllowed && driver.download && !driver.showDownloadResult) [driver cancelDownload:nil];
    if (driver.updater.automaticallyDownloadsUpdates != installsAllowed) driver.updater.automaticallyDownloadsUpdates=installsAllowed;
    if (driver.updater.automaticallyChecksForUpdates != checksAllowed) driver.updater.automaticallyChecksForUpdates=checksAllowed;
    if (driver.updater.updateCheckInterval != interval) driver.updater.updateCheckInterval=interval;
}
bool jaywalk_updater_check(void) {
    if (!initialize()) return false;
    if (driver.pendingRelaunch) {
        NSAlert *alert=[NSAlert new];alert.messageText=@"Restart to finish updating?";
        alert.informativeText=@"Finish waiting CLI sessions and restore local review checkouts before restarting.";
        [alert addButtonWithTitle:@"Later"];[alert addButtonWithTitle:@"Restart to update"];
        if ([alert runModal] == NSAlertSecondButtonReturn && jaywalk_updater_restart_is_safe()) {void (^handler)(void)=driver.pendingRelaunch;driver.pendingRelaunch=nil;handler();}
        return true;
    }
    [driver.updater checkForUpdates];return true;
}
const char *jaywalk_updater_status(void) {return lastStatus.UTF8String;}

const char *jaywalk_updater_feed(void) {
    NSString *feed=[NSBundle.mainBundle objectForInfoDictionaryKey:@"SUFeedURL"];
    return feed.length ? feed.UTF8String : "No public update feed is configured in this build.";
}
