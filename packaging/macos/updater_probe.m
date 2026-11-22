// Runs without windows, requests or credentials. Real signed upgrades are separate.
#import "updater.m"
#include <assert.h>
bool jaywalk_updater_restart_is_safe(void) {return true;}
int main(void) {
    @autoreleasepool {
        jaywalk_updater_configure(false,true,true,86400,true);
        assert(driver == nil && !checksAllowed && !downloadsAllowed && !installsAllowed);
        assert(!jaywalk_updater_check()); // No feed/key in this probe's bundle.
        jaywalk_updater_configure(false,false,false,86400,true);
        assert(driver == nil && !checksAllowed);
        jaywalk_updater_configure(true,false,false,86400,true);
        assert(driver == nil && checksAllowed && !downloadsAllowed && !installsAllowed);
        assert([JaywalkUpdateDriver allowedURL:[NSURL URLWithString:@"https://github.com/sorahn/jaywalk/releases/download/v1.2.3/update.zip"]]);
        assert(![JaywalkUpdateDriver allowedURL:[NSURL URLWithString:@"http://github.com/sorahn/jaywalk/releases/download/v1.2.3/update.zip"]]);
        assert(![JaywalkUpdateDriver allowedURL:[NSURL URLWithString:@"https://github.com/other/repository/releases/download/v1.2.3/update.zip"]]);
        assert(![JaywalkUpdateDriver allowedURL:[NSURL URLWithString:@"https://github.com.evil.invalid/sorahn/jaywalk/releases/download/v1.2.3/update.zip"]]);
        NSURL *temporary=[NSURL fileURLWithPath:[NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString] isDirectory:YES];
        NSFileManager *files=NSFileManager.defaultManager;
        assert([files createDirectoryAtURL:temporary withIntermediateDirectories:YES attributes:nil error:nil]);
        NSURL *cache=[temporary URLByAppendingPathComponent:@"cache" isDirectory:YES];
        NSURL *incoming=[temporary URLByAppendingPathComponent:@"incoming.zip"];
        NSDictionary *oldMetadata=@{@"version":@"old",@"url":@"old-url"};
        NSDictionary *newMetadata=@{@"version":@"new",@"url":@"new-url"};
        NSData *good=[@"good archive" dataUsingEncoding:NSUTF8StringEncoding];
        assert([good writeToURL:incoming atomically:YES]);
        NSError *error=nil;
        assert(saveDownloadedArchive(incoming,cache,good.length,oldMetadata,&error));
        NSURL *archive=[cache URLByAppendingPathComponent:@"downloaded-update.zip"];
        NSURL *receipt=[cache URLByAppendingPathComponent:@"download.plist"];
        NSData *shortData=[@"short" dataUsingEncoding:NSUTF8StringEncoding];
        assert([shortData writeToURL:incoming atomically:YES]);
        assert(!saveDownloadedArchive(incoming,cache,good.length,newMetadata,&error));
        assert(error.code == 2);
        assert([[NSData dataWithContentsOfURL:archive] isEqual:good]);
        assert([[NSDictionary dictionaryWithContentsOfURL:receipt error:nil] isEqual:oldMetadata]);
        // Oversized responses also fail; a matching retry replaces the cache.
        error=nil;
        assert(!saveDownloadedArchive(incoming,cache,1,newMetadata,&error));
        assert([[NSData dataWithContentsOfURL:archive] isEqual:good]);
        assert([good writeToURL:incoming atomically:YES]);
        error=nil;
        assert(saveDownloadedArchive(incoming,cache,good.length,newMetadata,&error));
        assert([[NSDictionary dictionaryWithContentsOfURL:receipt error:nil] isEqual:newMetadata]);
        assert([files removeItemAtURL:temporary error:nil]);
        puts("Updater probe passed: no configuration/consent creates no updater; asset URLs constrained.");
    }
}
