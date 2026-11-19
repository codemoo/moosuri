# Prepare Google Workspace for Moosuri

## Run the command from the setup wizard

무수리의 **관리 → Google 커넥터**에서 복사한 명령을 아래 Cloud Shell 터미널에 붙여 넣고 Enter를 누릅니다. Google이 권한을 요청하면 프로젝트를 관리할 계정인지 확인하고 승인합니다.

The command uses your signed-in Google Cloud account. It opens the project ID shown in Moosuri, or creates it if needed, then enables and checks the Gmail, Drive, Docs, Slides and Sheets APIs. You can rerun the command with the same ID if you close the tab or a request is interrupted. It does not change your default gcloud project.

If your organization requires a particular folder or organization for new projects, append `--folder=NUMBER` or `--organization=NUMBER` using the numeric parent ID from your administrator.

## Return to Moosuri

터미널에 **Ready**가 표시되면 무수리 탭으로 돌아갑니다. 같은 프로젝트에서 Google의 동의 화면을 설정하고 웹 애플리케이션 클라이언트를 만듭니다. 마법사가 표시한 콜백 주소를 등록하고 Google에서 받은 웹 클라이언트 JSON을 무수리 관리자 화면에서 선택합니다. 일반 회원은 각자 Google 계정으로 동의합니다.

[Google Workspace setup](google.md) · [Google Cloud project creation](https://cloud.google.com/resource-manager/docs/creating-managing-projects) · [Google Service Usage](https://cloud.google.com/service-usage/docs/enable-disable)
