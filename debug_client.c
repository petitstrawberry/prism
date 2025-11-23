#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// --- 定数定義 (Headers Fix) ---

// 'rout' (0x726F7574)
#define kAudioPrismPropertyRoutingTable 0x726F7574

// 'cust' (0x63757374) - 本来はドライバ用ヘッダーにある定数
#ifndef kAudioObjectPropertyCustomPropertyInfoList
#define kAudioObjectPropertyCustomPropertyInfoList 0x63757374
#endif

// デフォルトUID（Prism用）
#define DEFAULT_DEVICE_UID "com.petitstrawberry.driver.Prism.Device"

// -----------------------------

// Custom Property Info struct
typedef struct {
    AudioObjectPropertySelector mSelector;
    AudioObjectPropertySelector mPropertyDataType;
    AudioObjectPropertySelector mQualifierDataType;
} AudioServerPlugInCustomPropertyInfo;

void print_selector(AudioObjectPropertySelector s) {
    char str[5];
    // Big Endian convert for display
    UInt32 be = CFSwapInt32HostToBig(s);
    memcpy(str, &be, 4);
    str[4] = '\0';
    printf("'%s' (0x%X)", str, s);
}

int main(int argc, char** argv) {
    const char* device_uid = DEFAULT_DEVICE_UID;
    if (argc > 1) {
        device_uid = argv[1];
    }
    printf("--- Prism Debug Client (Deep Inspector v3) ---\n");

    // 1. Find Device by UID
    printf("Scanning for UID: %s ... ", device_uid);

    AudioObjectPropertyAddress addr_devs = { kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };
    UInt32 size = 0;
    AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr_devs, 0, NULL, &size);
    int count = size / sizeof(AudioObjectID);
    AudioObjectID* ids = (AudioObjectID*)malloc(size);
    AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr_devs, 0, NULL, &size, ids);

    AudioObjectID prismID = kAudioObjectUnknown;
    AudioObjectPropertyAddress addr_uid = { kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };

    for(int i=0; i<count; i++) {
        CFStringRef uid = NULL;
        UInt32 s = sizeof(CFStringRef);
        // エラーチェックを緩めてとにかく探す
        if(AudioObjectGetPropertyData(ids[i], &addr_uid, 0, NULL, &s, &uid) == 0 && uid != NULL) {
            char buf[128];
            if (CFStringGetCString(uid, buf, 128, kCFStringEncodingUTF8)) {
                // printf("Checking: %s\n", buf); // デバッグ用
                if(strcmp(buf, device_uid) == 0) {
                    prismID = ids[i];
                    CFRelease(uid);
                    break;
                }
            }
            CFRelease(uid);
        }
    }
    free(ids);

    if (prismID == kAudioObjectUnknown) {
        printf("\n❌ Not Found. (Device UID mismatch?)\n");
        return 1;
    }
    printf("✅ Found ID: %d\n", prismID);

    // 2. Inspect 'cust' (Custom Property List)
    printf("\n[Inspecting 'cust' Property]\n");
    // ★★★ 以下の遅延コードを追加 ★★★
printf("Waiting for HAL synchronization...\n");
// 100ms 程度の遅延を入れることで、HALがバックグラウンドでプロパティを読み込む猶予を与える
usleep(100000); // 100 milliseconds

    AudioObjectPropertyAddress addr_cust = {
        kAudioObjectPropertyCustomPropertyInfoList,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMaster
    };

    UInt32 custSize = 0;
    OSStatus err = AudioObjectGetPropertyDataSize(prismID, &addr_cust, 0, NULL, &custSize);

    if (err != 0) {
        printf("❌ Failed to get size of 'cust' list. Error: %d\n", err);
    } else {
        int numProps = custSize / sizeof(AudioServerPlugInCustomPropertyInfo);
        printf("Size: %d bytes (%d properties)\n", custSize, numProps);

        if (numProps > 0) {
            AudioServerPlugInCustomPropertyInfo* props = (AudioServerPlugInCustomPropertyInfo*)malloc(custSize);
            AudioObjectGetPropertyData(prismID, &addr_cust, 0, NULL, &custSize, props);

            for(int i=0; i<numProps; i++) {
                printf("  [%d] Selector: ", i);
                print_selector(props[i].mSelector);
                printf(", Type: ");
                print_selector(props[i].mPropertyDataType);
                printf("\n");
            }
            free(props);
        } else {
            printf("⚠️  List is EMPTY. Driver returned no properties.\n");
        }
    }

    // 3. Check 'rout' directly
    printf("\n[Checking 'rout']\n");
    AudioObjectPropertyAddress addr_rout = { kAudioPrismPropertyRoutingTable, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };

    Boolean hasRout = AudioObjectHasProperty(prismID, &addr_rout);
    printf("HasProperty('rout'): %s\n", hasRout ? "✅ TRUE" : "❌ FALSE");

    if (hasRout) {
        Boolean isSettable = 0;
        OSStatus err = AudioObjectIsPropertySettable(prismID, &addr_rout, &isSettable);
        printf("IsPropertySettable('rout'): %s (Err: %d)\n", isSettable ? "✅ YES" : "❌ NO", err);
    }

    // 4. Check standard properties
    printf("\n[Checking Standard Properties]\n");

    // Check Device Name ('lnam')
    AudioObjectPropertyAddress addr_name = { kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };
    CFStringRef name = NULL;
    UInt32 size_name = sizeof(CFStringRef);
    OSStatus err_name = AudioObjectGetPropertyData(prismID, &addr_name, 0, NULL, &size_name, &name);

    if (err_name == 0 && name != NULL) {
        char buf[128];
        CFStringGetCString(name, buf, 128, kCFStringEncodingUTF8);
        printf("Name ('lnam'): ✅ '%s'\n", buf);
        CFRelease(name);
    } else {
        printf("Name ('lnam'): ❌ FAILED (Error: %d)\n", err_name);
    }

    // Check Transport Type ('tran')
    AudioObjectPropertyAddress addr_tran = { kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };
    UInt32 transport = 0;
    UInt32 size_tran = sizeof(UInt32);
    OSStatus err_tran = AudioObjectGetPropertyData(prismID, &addr_tran, 0, NULL, &size_tran, &transport);

    if (err_tran == 0) {
        print_selector(transport);
        printf(" ('tran'): ✅ Success\n");
    } else {
        printf("Transport ('tran'): ❌ FAILED (Error: %d)\n", err_tran);
    }

    // Check Is Running ('ruin')
    AudioObjectPropertyAddress addr_run = { kAudioDevicePropertyDeviceIsRunning, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMaster };
    UInt32 running = 0;
    UInt32 size_run = sizeof(UInt32);
    OSStatus err_run = AudioObjectGetPropertyData(prismID, &addr_run, 0, NULL, &size_run, &running);

    if (err_run == 0) {
        printf("IsRunning ('ruin'): ✅ %s\n", running ? "TRUE" : "FALSE");
    } else {
        printf("IsRunning ('ruin'): ❌ FAILED (Error: %d)\n", err_run);
    }

    return 0;
}