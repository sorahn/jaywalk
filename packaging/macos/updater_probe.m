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
        puts("Updater probe passed: no configuration/consent creates no updater; asset URLs constrained.");
    }
}
