/* Standalone root-only PAM probe: never opens a session or executes a command.
 * Linux-PAM ABI declarations avoid a new runtime dependency or pam-devel need. */
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
typedef struct pam_handle pam_handle_t;
struct pam_message { int msg_style; const char *msg; };
struct pam_response { char *resp; int resp_retcode; };
struct pam_conv { int (*conv)(int,const struct pam_message **,struct pam_response **,void *); void *appdata_ptr; };
extern int pam_start_confdir(const char *,const char *,const struct pam_conv *,const char *,pam_handle_t **);
extern int pam_authenticate(pam_handle_t *,int);
extern int pam_acct_mgmt(pam_handle_t *,int);
extern int pam_setcred(pam_handle_t *,int);
extern int pam_end(pam_handle_t *,int);
static int converse(int n,const struct pam_message **messages,struct pam_response **out,void *data) {
    int password_mode=data?*(int *)data:0;
    if(n<=0 || n>32 || !out || !messages) return 19;
    for(int i=0;i<n;i++) {
        if(!messages[i]) return 19;
        if(messages[i]->msg_style!=3 && messages[i]->msg_style!=4 && !(password_mode && n==1 && messages[i]->msg_style==1)) return 19;
    }
    *out=calloc((size_t)n,sizeof(struct pam_response));
    if(!*out) return 5;
    for(int i=0;i<n;i++) {
        if(messages[i]->msg_style==1) {
            /* Only this explicit development mode answers the original PAM
             * password prompt; the default live face probe supplies no secret. */
            char *password=getpass(messages[i]->msg?messages[i]->msg:"PAM password: ");
            if(!password) { free(*out);*out=NULL;return 19; }
            (*out)[i].resp=strdup(password);
            volatile char *wipe=password;
            size_t length=strlen(password);
            for(size_t j=0;j<length;j++) wipe[j]=0;
            if(!(*out)[i].resp) { free(*out);*out=NULL;return 5; }
        } else fprintf(stderr,"PAM info: %s\n",messages[i]->msg?messages[i]->msg:"");
    }
    return 0;
}
int main(int argc,char **argv) {
    int password_mode=argc==5 && !strcmp(argv[4],"--password");
    if(getuid()!=0 || geteuid()!=0 || (argc!=4 && !password_mode)) { fputs("root only: kde-pam-probe SERVICE USER CONFDIR [--password]\n",stderr);return 1; }
    if(!strcmp(argv[1],"diagnose")) {
        /* Fixed installed diagnostic helper, never a user-supplied command. */
        execl("/usr/local/libexec/linux-hello-sudo/hello-sudo","hello-sudo","diagnose",argv[2],(char *)NULL);
        perror("diagnostic exec");return 1;
    }
    if(strcmp(argv[1],"plasmalogin") && strcmp(argv[1],"sddm") && strcmp(argv[1],"plasmalogin-greeter") && strcmp(argv[1],"plasmalogin-autologin")) return 1;
    struct pam_conv conv={converse,&password_mode}; pam_handle_t *handle=NULL;
    int status=pam_start_confdir(argv[1],argv[2],&conv,argv[3],&handle);
    if(status) { fprintf(stderr,"PAM_START=%d\n",status);return 1; }
    status=pam_authenticate(handle,0); fprintf(stderr,"PAM_AUTHENTICATE=%d\n",status);
    if(!status) { status=pam_acct_mgmt(handle,0);fprintf(stderr,"PAM_ACCT_MGMT=%d\n",status); }
    if(!status) { status=pam_setcred(handle,2);fprintf(stderr,"PAM_SETCRED=%d\n",status); }
    pam_end(handle,status); return status?1:0;
}
