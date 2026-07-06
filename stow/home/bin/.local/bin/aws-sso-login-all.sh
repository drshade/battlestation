#!/bin/bash

# Get all profiles from the aws config file
profiles=$(grep "^\\[profile" ~/.aws/config | sed -e "s/\\[profile //" -e "s/\\]//" && grep "^\\[default\\]" ~/.aws/config > /dev/null && echo "default")
echo $profiles
# Loop through the profiles and run aws sso login
for profile in $profiles
do
    echo "Logging in to profile: $profile"
    aws sso login --profile "$profile"
done

docker container run -it -v ~/.aws/:/root/.aws aws-sso-cred-restore