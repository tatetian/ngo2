#ifndef _OCCLUM_VERSION_H_
#define _OCCLUM_VERSION_H_

#define OCCLUM_MAJOR_VERSION    0
#define OCCLUM_MINOR_VERSION    13
#define OCCLUM_PATCH_VERSION    0

#define STRINGIZE_PRE(X) #X
#define STRINGIZE(X) STRINGIZE_PRE(X)

#define OCCLUM_VERSION_NUM_STR STRINGIZE(OCCLUM_MAJOR_VERSION) "." \
                    STRINGIZE(OCCLUM_MAJOR_VERSION) "." STRINGIZE(OCCLUM_PATCH_VERSION)

#endif
