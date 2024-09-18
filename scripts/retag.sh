#!/bin/bash

# Get the latest Git tag
latest_version=$(git describe --tags --abbrev=0 2>/dev/null)

# Exit if no tag is found
if [ -z "$latest_version" ]; then
    echo "Error: No Git tag found"
    exit 1
fi

echo "Latest tag: $latest_version"
read -p "Do you want to recreate and push this tag? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]
then
    echo "Operation cancelled"
    exit 1
fi

# Delete local tag
git tag -d $latest_version

# Delete remote tag
git push origin :refs/tags/$latest_version

# Create new local tag
git tag $latest_version

# Push new tag to remote
git push origin $latest_version

echo "Tag $latest_version has been recreated and pushed"
