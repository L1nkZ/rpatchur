Docker may be used to build RPatchur through containers.  Through Docker, the build process contains all necessary dependencies.  To build RPatchur with Docker, complete the following procedure.
```
# Build the RPatchur build container
cd /rpatchur/docker
docker image build -t rpatchur .
# Use the rpatchur image to build rpatchur
docker run -v /path/to/rpatchur_dir:/rpatchur rpatchur
```
